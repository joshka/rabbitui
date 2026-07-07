Output target/betamax/ratatui_interop.gif

Set Shell "bash"
Set FontSize 22
Set Width 900
Set Height 500
Set Padding 10
Set CursorBlink false
Set TypingSpeed 30ms
Set WaitTimeout 60s

Hide
Type "cd $HOME/local/rabbitui/work/default && cargo build -q -p rabbitui-ratatui --examples && echo TAPE_BUILD_READY"
Enter
Wait+Screen "TAPE_BUILD_READY"
Type "./target/debug/examples/ratatui_interop"
Show
Enter
Wait+Screen "rabbitui"
Sleep 300ms
Screenshot target/betamax/ratatui-interop.png
State target/betamax/ratatui-interop.json
Type "q"
Sleep 300ms
