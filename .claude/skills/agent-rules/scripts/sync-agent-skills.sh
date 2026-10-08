#!/bin/sh
# Two-way skill discovery between agent runtimes.
#
# A skill has exactly one real directory and one symlinked discovery path. The
# real directory may originate on either side; the opposite path is always a
# link to it, so an edit through either path reaches the same files and there is
# never a mirror to keep in sync by hand.
#
#   sync-agent-skills.sh            create/repair links, remove managed stale ones
#   sync-agent-skills.sh --check    report only, exit non-zero on any drift
#
# --check writes nothing, so it is safe in a pre-commit hook or in CI.
#
# Discovery roots default to .claude/skills and .agents/skills, relative to the
# git top level. Override with SYNC_PRIMARY_DIR / SYNC_SECONDARY_DIR (paths
# relative to the repository root) when a runtime uses different locations.
#
# Dependencies: git, a POSIX shell, and a filesystem that has symlinks.
#
# The last one is not decoration. Every skill here is one real directory and
# one link to it; there is no mirroring mode, so on a system that cannot make
# a symlink there is nothing for this script to do. Windows is that system
# unless Developer Mode is on or the shell is elevated: MSYS/Git Bash quietly
# COPIES instead of linking, `[ -L ]` is then false for the copy, --check
# reports drift that cannot be repaired, and a sync loops trying to repair it.
# So it is tested for once, up front, and refused out loud — a clear failure
# beats a silent one that looks like corruption.

set -eu

symlinks_work() {
    sw_d=$(mktemp -d 2>/dev/null) || return 1
    sw_ok=1
    ln -s target "$sw_d/link" 2>/dev/null && [ -L "$sw_d/link" ] || sw_ok=0
    rm -rf "$sw_d"
    [ "$sw_ok" = 1 ]
}

mode=sync
case "${1:-}" in
    "")       ;;
    --check)  mode=check ;;
    *)        printf 'usage: %s [--check]\n' "$0" >&2; exit 2 ;;
esac
[ $# -le 1 ] || { printf 'usage: %s [--check]\n' "$0" >&2; exit 2; }

symlinks_work || {
    printf 'error: this filesystem does not support symlinks, and this script is built on them.\n' >&2
    printf '  On Windows, either enable Developer Mode and set\n' >&2
    printf '    git config --global core.symlinks true\n' >&2
    printf '    export MSYS=winsymlinks:nativestrict\n' >&2
    printf '  or keep one discovery root and point the other runtime at it directly.\n' >&2
    exit 1
}

root=$(git rev-parse --show-toplevel 2>/dev/null) ||
    { printf 'error: not inside a git repository\n' >&2; exit 1; }

primary_rel="${SYNC_PRIMARY_DIR:-.claude/skills}"
secondary_rel="${SYNC_SECONDARY_DIR:-.agents/skills}"
primary="$root/$primary_rel"
secondary="$root/$secondary_rel"

status=0
fail() { printf '%s\n' "$*" >&2; status=1; }
rel() { printf '%s' "${1#"$root"/}"; }

# A link this script is allowed to touch: one pointing into either discovery
# root. Anything else was placed by a human and is left alone.
is_managed_link() {
    [ -L "$1" ] || return 1
    target=$(readlink "$1")
    case "$target" in
        "../../$primary_rel"/*|"../../$secondary_rel"/*) return 0 ;;
        *) return 1 ;;
    esac
}

# report_conflict is set only for the first scan direction, so a skill that is
# real on both sides is reported once rather than twice.
ensure_link() {
    discovery_path="$1"
    expected_target="$2"
    report_conflict="$3"

    if [ -L "$discovery_path" ]; then
        [ "$(readlink "$discovery_path")" = "$expected_target" ] && return 0
        if [ "$mode" = check ]; then
            fail "incorrect skill link: $(rel "$discovery_path")"
            return 0
        fi
        if is_managed_link "$discovery_path"; then
            rm -f "$discovery_path"
        else
            fail "refusing to replace unmanaged symlink: $(rel "$discovery_path")"
            return 0
        fi
    elif [ -e "$discovery_path" ]; then
        [ "$report_conflict" = yes ] &&
            fail "skill conflict: both discovery paths hold real content for $(basename "$discovery_path")"
        return 0
    elif [ "$mode" = check ]; then
        fail "missing skill link: $(rel "$discovery_path")"
        return 0
    fi

    ln -s "$expected_target" "$discovery_path"
}

# Every real skill directory under owner_root gets a link at the same name under
# discovery_root.
scan_real_skills() {
    owner_root="$1"
    discovery_root="$2"
    target_prefix="$3"
    report_conflicts="$4"

    for entry in "$owner_root"/*; do
        [ -e "$entry" ] || [ -L "$entry" ] || continue
        [ -L "$entry" ] && continue

        if [ ! -d "$entry" ] || [ ! -f "$entry/SKILL.md" ]; then
            fail "invalid skill directory (no SKILL.md): $(rel "$entry")"
            continue
        fi

        name=$(basename "$entry")
        ensure_link "$discovery_root/$name" "$target_prefix/$name" "$report_conflicts"
    done
}

# A link whose target no longer exists, or that was renamed out from under its
# target, is stale. Only managed links are removed; a real directory never is.
clean_or_check_links() {
    discovery_root="$1"
    expected_prefix="$2"

    for entry in "$discovery_root"/*; do
        [ -L "$entry" ] || continue
        name=$(basename "$entry")

        if [ "$(readlink "$entry")" = "$expected_prefix/$name" ] && [ -f "$entry/SKILL.md" ]; then
            continue
        fi

        if [ "$mode" = check ]; then
            fail "stale or misnamed skill link: $(rel "$entry")"
        elif is_managed_link "$entry"; then
            rm -f "$entry"
        else
            fail "refusing to remove unmanaged symlink: $(rel "$entry")"
        fi
    done
}

if [ "$mode" = check ]; then
    [ -d "$primary" ] || fail "missing discovery root: $primary_rel"
    [ -d "$secondary" ] || fail "missing discovery root: $secondary_rel"
else
    mkdir -p "$primary" "$secondary"
fi

if [ -d "$primary" ] && [ -d "$secondary" ]; then
    scan_real_skills "$primary" "$secondary" "../../$primary_rel" yes
    scan_real_skills "$secondary" "$primary" "../../$secondary_rel" no

    clean_or_check_links "$primary" "../../$secondary_rel"
    clean_or_check_links "$secondary" "../../$primary_rel"
fi

[ "$status" -eq 0 ] || exit "$status"

if [ "$mode" = check ]; then
    printf 'skills are in two-way sync (%s <-> %s)\n' "$primary_rel" "$secondary_rel"
else
    printf 'skills synchronized in both directions (%s <-> %s)\n' "$primary_rel" "$secondary_rel"
fi
