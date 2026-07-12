Output target/betamax/agent.gif

Set Shell "bash"
Set FontSize 22
Set Width 900
Set Height 500
Set Padding 10
Set CursorBlink false
Set TypingSpeed 30ms
Set WaitTimeout 60s

Hide
Type "cd $HOME/local/rabbitui/work/default && cargo build -q --examples && echo TAPE_BUILD_READY"
Enter
Wait+Screen "TAPE_BUILD_READY"
Type "./target/debug/examples/agent"
Show
Enter
# The composer is focused at startup (so you can type immediately), which hides
# its placeholder — so wait on the always-visible status word "idle", never on
# focus-dependent placeholder text.
Wait+Screen "idle"
Sleep 200ms
Type "hello rabbit"
Enter
Sleep 2s
Screenshot target/betamax/agent-streaming.png
Sleep 5s
Screenshot target/betamax/agent-done-inline.png
State target/betamax/agent-done-inline.json
# Ctrl-T toggles mode: bare "m" would be swallowed by the focused composer.
Ctrl+T
Sleep 500ms
Screenshot target/betamax/agent-altscreen.png
Ctrl+T
Sleep 500ms
# Ctrl-C quits (bare "q" is swallowed by the focused composer).
Ctrl+C
Sleep 300ms
