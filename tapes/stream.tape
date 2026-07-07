Output target/betamax/stream.gif

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
Type "./target/debug/examples/stream"
Show
Enter
Wait+Screen "committed"
Sleep 200ms
Type "nnn"
Sleep 400ms
Screenshot target/betamax/stream-commits.png
Type "m"
Sleep 400ms
Screenshot target/betamax/stream-altscreen.png
Type "m"
Sleep 400ms
Screenshot target/betamax/stream-back-inline.png
State target/betamax/stream-back-inline.json
Type "q"
Sleep 300ms
