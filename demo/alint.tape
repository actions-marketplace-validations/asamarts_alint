# alint README demo, as code.
#
# Regenerate:  vhs demo/alint.tape          (writes demo/alint.gif)
# Requires:    https://github.com/charmbracelet/vhs
#
# WHY a tape and not a hand-recorded GIF: alint is a drift-prevention tool, and a
# hand-recorded demo is precisely the kind of artifact that goes stale (an old
# version string, an output format that has since changed) with nothing to catch
# it. The tape is generated, reproducible, and re-runnable in CI, so the demo can
# never silently drift from what the binary actually prints.
#
# The recording runs against a scratch COPY of demo/fixture, so it never mutates
# the tracked fixture (the `fix` step rewrites files). The two artifacts git must
# not carry, a tracked `target/` and a `.DS_Store`, are synthesized here at record
# time rather than committed.

Output demo/alint.gif

Require alint
Require git

Set Shell bash
Set FontSize 14
Set Width 1100
Set Height 840
Set Padding 20
Set TypingSpeed 45ms
Set Theme "Catppuccin Mocha"

# --- setup (not recorded) --------------------------------------------------
Hide
Type "export PS1='$ ' && clear"
Enter
Type "cd $(mktemp -d) && cp -r ${ALINT_DEMO_FIXTURE:-$OLDPWD/demo/fixture}/. ."
Enter
# Synthesize the two artifacts we deliberately do not commit.
Type "mkdir -p target/debug && printf 'binary\n' > target/debug/acme && printf '\n' > .DS_Store"
Enter
Type "git init -q . && git add -A && clear"
Enter
Sleep 500ms
Show

# --- 1. the problem --------------------------------------------------------
Type "# A Rust repo with the usual structural drift. Nothing here checks for it."
Enter
Sleep 1.5s

Type "alint check --width 88 --no-docs"
Enter
Sleep 6s

# --- 2. fix what is mechanical ----------------------------------------------
# NOTE: the narration deliberately does NOT restate the violation counts. The
# output already shows them, and a hard-coded "3 errors, 4 fixable" in the
# script is precisely the kind of claim that goes stale the next time a bundled
# rule changes. Let the tool speak for itself.
Type "# Some of that is mechanical. alint fixes it in place."
Enter
Sleep 1.5s

Type "alint fix --width 88"
Enter
Sleep 5s

# --- 3. what is left is a human decision ------------------------------------
Type "alint check --width 88 --no-docs --compact"
Enter
Sleep 4s

Type "# What remains is structural. That needs a decision, not a rewrite."
Enter
Sleep 2.5s
