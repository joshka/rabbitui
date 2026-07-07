Output target/betamax/todo.gif

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
Type "./target/debug/examples/todo"
Show
Enter
Wait+Screen "Add a todo"
Sleep 200ms
Type "buy carrots"
Sleep 200ms
Enter
Type "feed rabbit"
Enter
Sleep 200ms
Screenshot target/betamax/todo-two.png
Tab
Down
Sleep 200ms
Screenshot target/betamax/todo-selected.png
State target/betamax/todo-selected.json
Type "d"
Sleep 200ms
Screenshot target/betamax/todo-deleted.png
Escape
Sleep 300ms
