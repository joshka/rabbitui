Output target/betamax/focus.gif

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
Type "./target/debug/examples/focus"
Show
Enter
Wait+Screen "activate"
Sleep 200ms
Screenshot target/betamax/focus-initial.png
Tab
Sleep 200ms
Screenshot target/betamax/focus-tab.png
Enter
Sleep 200ms
Screenshot target/betamax/focus-activated.png
State target/betamax/focus-activated.json
Type "q"
Sleep 300ms
