//! `GitLance mcp`: the MCP server an agent runs in its chat (stdio, one JSON-RPC message per line).
//! It reads the review comments GitLance sent for the repository the agent works in and records
//! the agent's answers (`crate::agents`); it never writes the comments or the repository.

use crate::agents::{self, Answer, Bus, Kind};
use serde_json::{Value, json};
use std::io::{BufRead, Write};
use std::path::PathBuf;

const PROTOCOL: &str = "2025-06-18";

/// What the agent is told when the server connects.
fn instructions(exe: &str) -> String {
    format!(
        "GitLance is the reviewer's diff app. The reviewer writes comments on lines there and sends them to a chat. \
So that they reach this chat, start the watcher once, in the background, as soon as you work in a git repository: \
run `\"{exe}\" agent-watch` with the Monitor tool if you have it, otherwise with Bash and run_in_background. \
Do not wait for it. When it prints (review comments arrived), call list_comments, change the code for each comment, \
call address_comment with its id and one line on what you did — or reply_comment to ask the reviewer — then start the \
watcher again. Never claim a comment is resolved: the reviewer confirms each one in GitLance."
    )
}

fn tools() -> Value {
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
            "description": "Answer a review comment without saying it is done: a question for the reviewer, or why you did not change it.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "id": {"type": "integer"},
                    "text": {"type": "string"}
                },
                "required": ["id", "text"]
            }
        }
    ])
}

/// The repository the agent works in, named as the app names it (`git::Repo::root`).
fn repo() -> Result<PathBuf, String> {
    let cwd = std::env::current_dir().map_err(|e| e.to_string())?;
    crate::git::Repo::open(&cwd)
        .map(|r| r.root())
        .map_err(|_| format!("{} is not in a git repository", cwd.display()))
}

fn call(bus: &Bus, name: &str, args: &Value) -> Result<String, String> {
    let root = repo()?;
    let comments = bus.comments(&root);
    let open: Vec<_> = comments
        .iter()
        .filter(|c| c.state == crate::review::State::Sent)
        .collect();
    if name == "list_comments" {
        return Ok(agents::brief(&root, &open));
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
        at: agents::now(),
    };
    bus.answer(&root, &answer).map_err(|e| e.to_string())?;
    Ok(match kind {
        Kind::Addressed => format!("Comment {id} marked done; the reviewer will check it."),
        _ => format!("Reply to comment {id} sent to the reviewer."),
    })
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
            match call(bus, name, &params["arguments"]) {
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
        assert_eq!(list["result"]["tools"].as_array().unwrap().len(), 3);
        assert!(handle(&bus, "", &json!({"method": "notifications/initialized"})).is_none());
        let unknown = handle(&bus, "", &json!({"id": 3, "method": "nope"})).unwrap();
        assert_eq!(unknown["error"]["code"], -32601);
    }
}
