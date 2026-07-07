Output target/betamax/hello.gif

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
Type "./target/debug/examples/hello"
Show
Enter
Wait+Screen "rabbitui"
Sleep 300ms
Screenshot target/betamax/hello.png
State target/betamax/hello.json
Type "q"
Sleep 300ms
