Source docs/recordings/settings.tape
Output docs/images/search.gif
Set Height 1000

Hide
Type "cd /tmp && snomed-ecl-engine use uk > /dev/null && PS1='\n$ ' && clear"
Enter
Show

Type "snomed-ecl-engine search chronic kidney stage 3 --limit 6"
Sleep 400ms
Enter
Sleep 3.5s

Ctrl+L
Type "snomed-ecl-engine lookup 700378005"
Sleep 400ms
Enter
Sleep 6s

Ctrl+L
Type "snomed-ecl-engine history 155574008"
Sleep 400ms
Enter
Sleep 3s
