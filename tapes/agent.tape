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
Wait+Screen "prompt"
Sleep 200ms
Type "hello rabbit"
Enter
Sleep 2s
Screenshot target/betamax/agent-streaming.png
Sleep 5s
Screenshot target/betamax/agent-done-inline.png
State target/betamax/agent-done-inline.json
Type "m"
Sleep 500ms
Screenshot target/betamax/agent-altscreen.png
Type "m"
Sleep 500ms
Type "q"
Sleep 300ms
