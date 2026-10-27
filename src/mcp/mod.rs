//! `GitLance mcp`: the MCP server an agent runs in its chat (stdio, one JSON-RPC message per line).
//! It reads the review comments GitLance sent for the repository the agent works in and records
//! the agent's answers (`crate::agents`); it never writes the comments or the repository. It also
//! reads a merge request and keeps the agent's pending comments on it (`requests`, #150), and opens
//! what the agent changed for the reviewer (`changes`, #149).

mod changes;
mod requests;

use crate::agents::{self, Answer, Bus, Kind};
use serde_json::{Value, json};
use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

const PROTOCOL: &str = "2025-06-18";

/// What the agent is told when the server connects.
fn instructions(exe: &str) -> String {
    format!(
        "GitLance is the reviewer's diff app. The reviewer writes comments on lines there and sends them to a chat. \
So that they reach this chat, start the watcher once, in the background, as soon as you work in a git repository: \
run `\"{exe}\" agent-watch` with the Monitor tool if you have it, otherwise with Bash and run_in_background. \
Do not wait for it. When it prints (review comments arrived), call list_comments, change the code for each comment, \
call address_comment with its id and one line on what you did — or reply_comment to ask the reviewer — then start the \
watcher again. Read each comment for what it wants: a question gets an answer (reply_comment); a clear change gets made \
(then address_comment); a change that is large or unclear is asked about first, with a form (reply_comment with \
`form`: a plan to approve, a confirm, versions of code …); after explaining, you may offer the fix as a confirm. \
Never claim a comment is resolved: the reviewer confirms each one in GitLance. \
When GitLance itself fails (one of these tools, or the app), you may draft a report of it with report_problem; it only \
opens in the app for the reviewer to check and send. \
To review a merge request, read it with get_request — its diff as GitLab or GitHub compares it, its discussions \
and the pending comments already there — and leave each remark with add_pending_comment on a line the diff shows. When you have looked at several, \
gather them with gather_requests, each with a line on what you did there, so the reviewer finds them together. \
When you finish work the reviewer should read, open it for them with open_changes: your branch against its base, \
or the working tree. Where a change needs explaining — why this way, what you left alone, what to look at closely — \
give notes on its lines and a summary, with open_changes or alone with annotate_changes; answers to them come back as \
review comments. \
Pending comments are seen by the reviewer alone; the reviewer edits, deletes or keeps them and submits the review: \
never submit, approve or publish anything yourself. \
GitLance shows the reviewer what you did only while it runs. When your work needs the reviewer to look and a tool \
says GitLance is not running, call open_gitlance: it starts GitLance on this repository (or opens it in a tab) and \
answers once it is there. Do not start it for anything else."
    )
}

fn tools() -> Value {
    // A note on a line of the changes, for annotate_changes and open_changes (#151).
    let notes = json!({
        "type": "array",
        "description": "Notes on lines of the changes: a file they change and a line of its new side (the working tree's, or the head's)",
        "items": {
            "type": "object",
            "properties": {
                "path": {"type": "string", "description": "The file, from the repository's root"},
                "line": {"type": "integer", "description": "The line's number in the file as the changes leave it"},
                "text": {"type": "string", "description": "The note, in markdown: why, what was left alone, what to check"}
            },
            "required": ["path", "line", "text"]
        }
    });
    json!([
        {
            "name": "list_comments",
            "description": "The review comments the reviewer sent from GitLance for this repository that are not done yet: file, line in the working tree, the commented code, what to change.",
            "inputSchema": {"type": "object", "properties": {}}
        },
        {
            "name": "address_comment",
            "description": "Say a review comment is done, with one line on what you changed. The reviewer then checks and resolves it in GitLance.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "id": {"type": "integer", "description": "The comment's id from list_comments"},
                    "note": {"type": "string", "description": "What you changed, in one line"}
                },
                "required": ["id", "note"]
            }
        },
        {
            "name": "reply_comment",
            "description": "Answer a review comment without saying it is done: a question for the reviewer, or why you did not change it. To ask, give a form: the reviewer answers in GitLance with a click, and the answer comes with the comment the next time it is sent.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "id": {"type": "integer"},
                    "text": {"type": "string", "description": "The question or the reason, in a sentence or two"},
                    "form": {
                        "type": "object",
                        "description": "Optional. How the reviewer answers: one (pick one), many (pick any), confirm (yes / no; options give the two labels), plan (steps to approve, each on or off), code (versions of code to pick from, each with `code` lines), files (files to touch), text (a name, with suggestions), order (put the items in order), scale (how far: this function … everywhere), line (which line). The reviewer can always say \"your call\" or add a note.",
                        "properties": {
                            "kind": {"type": "string", "enum": ["one", "many", "confirm", "plan", "code", "files", "text", "order", "scale", "line"]},
                            "options": {
                                "type": "array",
                                "items": {
                                    "type": "object",
                                    "properties": {
                                        "label": {"type": "string"},
                                        "detail": {"type": "string"},
                                        "code": {"type": "array", "items": {"type": "string"}},
                                        "on": {"type": "boolean", "description": "Checked to begin with (plan, files)"}
                                    },
                                    "required": ["label"]
                                }
                            }
                        },
                        "required": ["kind"]
                    }
                },
                "required": ["id", "text"]
            }
        },
        {
            "name": "get_request",
            "description": "Read a merge request of this repository as GitLance shows it: title, description, source and target, the diff GitLab or GitHub compares (each line with its old and new number), the discussions, and the pending comments already there. A long diff lists the files it leaves out: ask for them with paths.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "iid": {"type": "integer", "description": "The request's number (!123, #123)"},
                    "paths": {"type": "array", "items": {"type": "string"}, "description": "Optional: only these files' diff"}
                },
                "required": ["iid"]
            }
        },
        {
            "name": "add_pending_comment",
            "description": "Leave a pending comment on a line of a merge request's diff, as the reviewer's own pending comments are written. Only the reviewer sees it until they submit the review; they can edit or delete it. A line not in the diff is refused with the lines that are. Answers the comment's id and place.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "iid": {"type": "integer"},
                    "path": {"type": "string", "description": "The file, as get_request names it"},
                    "line": {"type": "integer", "description": "The line's new number; for a removed line its old number, with old: true"},
                    "old": {"type": "boolean", "description": "line is an old number: a removed line"},
                    "start_line": {"type": "integer", "description": "Optional: a range from this line down to line, on the same side"},
                    "body": {"type": "string", "description": "The comment, in markdown"}
                },
                "required": ["iid", "path", "line", "body"]
            }
        },
        {
            "name": "gather_requests",
            "description": "Put the merge requests you worked on under Merge requests in GitLance's menu, in this order, each with a line on what you did there, for the reviewer to read one by one. Answers which are there and which numbers are not merge requests of this project.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "requests": {
                        "type": "array",
                        "items": {
                            "type": "object",
                            "properties": {
                                "iid": {"type": "integer", "description": "The request's number"},
                                "note": {"type": "string", "description": "What you did there: \"5 pending comments\", \"looked, nothing to say\""}
                            },
                            "required": ["iid"]
                        }
                    }
                },
                "required": ["requests"]
            }
        },
        {
            "name": "open_changes",
            "description": "Open what you changed for the reviewer, as a row under Branches in GitLance: two refs or commits compared (head against base, since their merge base unless direct), or the working tree. Nothing on their screen moves; a note offers to show it. With notes and a summary, they show on the diff as annotate_changes leaves them. Answers what it opened, or why it could not.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "base": {"type": "string", "description": "What the changes are compared against; the project's main line when left out"},
                    "head": {"type": "string", "description": "The changes; the branch checked out here when left out"},
                    "direct": {"type": "boolean", "description": "Compare the two as they are, not since their merge base"},
                    "working_tree": {"type": "boolean", "description": "The uncommitted changes instead of two refs"},
                    "unstaged": {"type": "boolean", "description": "With working_tree: only what is not staged"},
                    "summary": {"type": "string", "description": "Optional: what the changes do, in a few sentences, shown over the diff"},
                    "notes": notes
                }
            }
        },
        {
            "name": "annotate_changes",
            "description": "Explain your changes to the reviewer with notes on their lines — why this way, what you left alone, what to look at closely — and a short summary of the whole. They show on the diff in GitLance, apart from the reviewer's comments, whenever the reviewer opens these changes; a note answered comes back to you as an ordinary review comment. Kept on the reviewer's Mac, never sent to GitLab or GitHub. Name the changes as open_changes does: the working tree, or a head (your branch) against its base. Notes again on the same changes replace your earlier ones.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "base": {"type": "string", "description": "What the changes are compared against; the project's main line when left out"},
                    "head": {"type": "string", "description": "The changes; the branch checked out here when left out"},
                    "direct": {"type": "boolean", "description": "Compare the two as they are, not since their merge base"},
                    "working_tree": {"type": "boolean", "description": "The uncommitted changes instead of two refs"},
                    "unstaged": {"type": "boolean", "description": "With working_tree: only what is not staged"},
                    "summary": {"type": "string", "description": "What the changes do, in a few sentences, shown over the diff"},
                    "notes": notes
                },
                "required": ["notes"]
            }
        },
        {
            "name": "open_gitlance",
            "description": "Start GitLance on this repository, or open the repository in a tab when GitLance runs without it. Answers once it is there. Only when your work needs the reviewer to look and GitLance is not showing it.",
            "inputSchema": {"type": "object", "properties": {}}
        },
        {
            "name": "report_problem",
            "description": "Draft a report of a problem in GitLance itself (an error from the app or from these tools, something it did wrong) for its developers. Nothing is sent: the report opens in GitLance with your text, and the reviewer checks it and sends it, or not. Only GitLance's own problems, never the reviewer's code.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "title": {"type": "string", "description": "The problem in a few words"},
                    "description": {"type": "string", "description": "What happened, what was done just before, the error's text"}
                },
                "required": ["title", "description"]
            }
        }
    ])
}

/// The folder the agent works in and its project, named as the app names them
/// (`git::Repo::root`, `main_root`): a worktree's notes are its main working tree's, its lines its
/// own (#77).
fn repo() -> Result<(PathBuf, PathBuf), String> {
    let cwd = std::env::current_dir().map_err(|e| e.to_string())?;
    crate::git::Repo::open(&cwd)
        .map(|r| (r.root(), r.main_root()))
        .map_err(|_| format!("{} is not in a git repository", cwd.display()))
}

fn call(bus: &Bus, exe: &str, name: &str, args: &Value) -> Result<String, String> {
    match name {
        "report_problem" => return draft_report(bus, args),
        "open_gitlance" => return open_app(bus, exe, Duration::from_secs(30)),
        _ => {}
    }
    let (here, root) = repo()?;
    match name {
        "get_request" => return requests::get_request(bus, &here, &root, args),
        "add_pending_comment" => return requests::add_pending_comment(bus, &here, &root, args),
        "gather_requests" => return requests::gather_requests(bus, &here, &root, args),
        "open_changes" => return changes::open_changes(bus, &here, &root, args),
        "annotate_changes" => return changes::annotate_changes(bus, &here, &root, args),
        _ => {}
    }
    let comments = bus.comments(&root);
    // A chat's own notes, and those sent to no chat in particular (#77).
    let me = agents::whoami().map(|(_, session, ..)| session);
    let open: Vec<_> = comments
        .iter()
        .filter(|c| c.state == crate::review::State::Sent)
        .filter(|c| {
            let to = c.sent.as_ref().map_or("", |s| s.session.as_str());
            me.as_deref().is_none_or(|me| to.is_empty() || to == me)
        })
        .collect();
    if name == "list_comments" {
        return Ok(agents::brief(&here, &open));
    }
    let id = args["id"].as_u64().ok_or("id is missing")?;
    let comment = open
        .iter()
        .find(|c| c.id == id)
        .ok_or(format!("comment {id} is not waiting for an agent"))?;
    let (kind, body) = match name {
        "address_comment" => (Kind::Addressed, args["note"].as_str().unwrap_or_default()),
        "reply_comment" => (Kind::Reply, args["text"].as_str().unwrap_or_default()),
        other => return Err(format!("no tool {other}")),
    };
    let answer = Answer {
        uid: agents::uid(),
        id,
        batch: comment
            .sent
            .as_ref()
            .map(|s| s.batch.clone())
            .unwrap_or_default(),
        kind,
        body: body.to_owned(),
        // A reply may ask with a form; one that does not parse is sent as the text alone.
        form: (kind == Kind::Reply)
            .then(|| serde_json::from_value(args["form"].clone()).ok())
            .flatten(),
        at: agents::now(),
    };
    bus.answer(&root, &answer).map_err(|e| e.to_string())?;
    Ok(match kind {
        Kind::Addressed => format!("Comment {id} marked done; the reviewer will check it."),
        _ => format!("Reply to comment {id} sent to the reviewer."),
    })
}

/// A report of GitLance's own problem, drafted for the reviewer to check in the app (#118).
fn draft_report(bus: &Bus, args: &Value) -> Result<String, String> {
    let text = |key: &str| args[key].as_str().unwrap_or_default().trim().to_owned();
    let (title, description) = (text("title"), text("description"));
    if title.is_empty() {
        return Err("title is missing".to_owned());
    }
    let agent = agents::agent_name();
    let folder = std::env::current_dir()
        .ok()
        .and_then(|d| d.file_name().map(|n| n.to_string_lossy().into_owned()));
    let by = match folder {
        Some(folder) => format!("{agent} · {folder}"),
        None => agent,
    };
    let draft = agents::ReportDraft {
        title,
        description,
        by,
        at: agents::now(),
    };
    bus.draft_report(&draft).map_err(|e| e.to_string())?;
    Ok("The report opens in GitLance for the reviewer to check; nothing was sent.".to_owned())
}

/// What a tool adds when the reviewer cannot see its work yet: GitLance is not running (#153).
fn unseen(bus: &Bus) -> &'static str {
    if bus.running().is_empty() {
        "\n\nGitLance is not running, so the reviewer does not see this yet; it waits there. If they should \
         look now, call open_gitlance."
    } else {
        ""
    }
}

/// `open_gitlance` (#153): GitLance on this project — opened in a tab when the app runs without
/// it, started on it when the app does not run — answered once one of its windows says it is there.
fn open_app(bus: &Bus, exe: &str, wait: Duration) -> Result<String, String> {
    let (_, project) = repo()?;
    let name = crate::labels::project_name(&project);
    let shows = |bus: &Bus| bus.running().iter().any(|p| p.projects.contains(&project));
    if shows(bus) {
        return Ok(format!("GitLance has {name} open."));
    }
    let started = bus.running().is_empty();
    if started {
        crate::platform::launch(Path::new(exe), &project)
            .map_err(|e| format!("GitLance could not be started: {e}"))?;
    } else {
        bus.event(&agents::Event::new(project.clone(), agents::What::Open))
            .map_err(|e| e.to_string())?;
    }
    let until = Instant::now() + wait;
    while Instant::now() < until {
        if shows(bus) {
            return Ok(if started {
                format!("GitLance started on {name}. Call the tool you wanted again.")
            } else {
                format!("GitLance opened {name} in a tab. Call the tool you wanted again.")
            });
        }
        std::thread::sleep(Duration::from_millis(500));
    }
    Err(format!(
        "GitLance did not show {name} within {} s; the reviewer may have to open it.",
        wait.as_secs()
    ))
}

/// The answer to one request, or `None` for a notification.
fn handle(bus: &Bus, exe: &str, message: &Value) -> Option<Value> {
    let id = message.get("id")?.clone();
    let result = match message["method"].as_str().unwrap_or_default() {
        "initialize" => json!({
            "protocolVersion": message["params"]["protocolVersion"].as_str().unwrap_or(PROTOCOL),
            "capabilities": {"tools": {}},
            "serverInfo": {"name": "gitlance", "version": env!("CARGO_PKG_VERSION")},
            "instructions": instructions(exe),
        }),
        "ping" => json!({}),
        "tools/list" => json!({"tools": tools()}),
        "tools/call" => {
            let params = &message["params"];
            let name = params["name"].as_str().unwrap_or_default();
            match call(bus, exe, name, &params["arguments"]) {
                Ok(text) => json!({"content": [{"type": "text", "text": text}]}),
                Err(text) => json!({"content": [{"type": "text", "text": text}], "isError": true}),
            }
        }
        method => {
            return Some(json!({
                "jsonrpc": "2.0", "id": id,
                "error": {"code": -32601, "message": format!("no method {method}")}
            }));
        }
    };
    Some(json!({"jsonrpc": "2.0", "id": id, "result": result}))
}

pub fn run() {
    let Some(bus) = Bus::new() else {
        eprintln!("gitlance mcp: no home folder");
        return;
    };
    let exe = std::env::current_exe()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|_| "gitlance".into());
    let stdin = std::io::stdin();
    let mut stdout = std::io::stdout();
    for line in stdin.lock().lines() {
        let Ok(line) = line else { break };
        let Ok(message) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        if let Some(reply) = handle(&bus, &exe, &message) {
            let _ = writeln!(stdout, "{reply}");
            let _ = stdout.flush();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn it_introduces_itself_and_its_tools() {
        let bus = Bus::at(std::env::temp_dir().join("gitlance-mcp-test"));
        let init = handle(
            &bus,
            "/x/GitLance",
            &json!({"jsonrpc": "2.0", "id": 1, "method": "initialize",
                    "params": {"protocolVersion": "2025-03-26"}}),
        )
        .unwrap();
        assert_eq!(init["result"]["protocolVersion"], "2025-03-26");
        assert!(
            init["result"]["instructions"]
                .as_str()
                .unwrap()
                .contains("\"/x/GitLance\" agent-watch")
        );
        let list = handle(&bus, "", &json!({"id": 2, "method": "tools/list"})).unwrap();
        assert_eq!(list["result"]["tools"].as_array().unwrap().len(), 10);
        assert!(handle(&bus, "", &json!({"method": "notifications/initialized"})).is_none());
        let unknown = handle(&bus, "", &json!({"id": 3, "method": "nope"})).unwrap();
        assert_eq!(unknown["error"]["code"], -32601);
    }

    #[test]
    fn gitlance_is_opened_on_this_project_only_when_it_does_not_show_it() {
        let dir = tempfile::TempDir::new().unwrap();
        let bus = Bus::at(dir.path().to_path_buf());
        let (_, project) = repo().unwrap();
        let window = |projects: Vec<PathBuf>| agents::Presence {
            pid: std::process::id(),
            projects,
            beat: agents::now(),
        };
        bus.present(1, &window(vec![project.clone()])).unwrap();
        let shown = open_app(&bus, "", Duration::ZERO).unwrap();
        assert!(shown.contains("has"), "{shown}");
        assert!(bus.events().is_empty());
        // Running without it: the app is asked to open it, and the answer waits for that.
        bus.present(1, &window(Vec::new())).unwrap();
        let waited = open_app(&bus, "", Duration::from_millis(600)).unwrap_err();
        assert!(waited.contains("did not show"), "{waited}");
        let events = bus.events();
        assert_eq!(events.len(), 1);
        assert_eq!(
            (&events[0].project, &events[0].what),
            (&project, &agents::What::Open)
        );
    }

    #[test]
    fn a_report_is_only_drafted() {
        let dir = tempfile::TempDir::new().unwrap();
        let bus = Bus::at(dir.path().to_path_buf());
        let call = |args: Value| {
            handle(
                &bus,
                "",
                &json!({"id": 4, "method": "tools/call",
                        "params": {"name": "report_problem", "arguments": args}}),
            )
            .unwrap()
        };
        let none = call(json!({"title": " ", "description": "x"}));
        assert_eq!(none["result"]["isError"], true);
        let done =
            call(json!({"title": "list_comments failed", "description": "It said: no home."}));
        assert!(
            done["result"]["content"][0]["text"]
                .as_str()
                .unwrap()
                .contains("nothing was sent")
        );
        let drafts = bus.take_reports();
        assert_eq!(drafts.len(), 1);
        assert_eq!(drafts[0].title, "list_comments failed");
        assert!(bus.take_reports().is_empty(), "taken once");
    }
}
