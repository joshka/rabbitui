Output target/betamax/counter.gif

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
Type "./target/debug/examples/counter"
Show
Enter
Wait+Screen "press"
Sleep 200ms
Screenshot target/betamax/counter-initial.png
Type "+++"
Sleep 200ms
Screenshot target/betamax/counter-plus3.png
State target/betamax/counter-plus3.json
Type "-"
Sleep 200ms
Type "q"
Sleep 300ms
