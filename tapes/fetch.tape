Output target/betamax/fetch.gif

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
Type "./target/debug/examples/fetch"
Show
Enter
Wait+Screen "search"
Sleep 200ms
Type "carrot"
Sleep 1s
Screenshot target/betamax/fetch-searching.png
Sleep 2s
Screenshot target/betamax/fetch-results.png
State target/betamax/fetch-results.json
Ctrl+C
Sleep 300ms
