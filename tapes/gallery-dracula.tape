Output target/betamax/gallery-dracula.gif

Set Shell "bash"
Set FontSize 20
Set Width 900
Set Height 820
Set Padding 10
Set CursorBlink false
Set TypingSpeed 30ms
Set WaitTimeout 60s

Hide
Type "cd $HOME/local/rabbitui/work/default && cargo build -q --examples && echo TAPE_BUILD_READY"
Enter
Wait+Screen "TAPE_BUILD_READY"
Type "GALLERY_THEME=dracula ./target/debug/examples/gallery"
Show
Enter
Wait+Screen "rabbitui gallery"
Sleep 400ms
Screenshot target/betamax/gallery-dracula-top.png
Down
Down
Down
Down
Down
Down
Down
Down
Sleep 400ms
Screenshot target/betamax/gallery-dracula-roles.png
Type "q"
Sleep 200ms
