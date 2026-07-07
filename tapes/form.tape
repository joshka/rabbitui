Output target/betamax/form.gif

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
Type "./target/debug/examples/form"
Show
Enter
Wait+Screen "Enter: submit"
Sleep 200ms
Screenshot target/betamax/form-initial.png
Type "Josh"
Tab
Type "josh@example.com"
Sleep 200ms
Screenshot target/betamax/form-filled.png
Tab 2
Enter
Sleep 300ms
Screenshot target/betamax/form-modal.png
State target/betamax/form-modal.json
Enter
Sleep 300ms
Screenshot target/betamax/form-submitted.png
Escape
Sleep 300ms
