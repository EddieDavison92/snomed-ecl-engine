Source docs/recordings/settings.tape
Output docs/images/expand.gif
Set Height 580

Hide
Type "cd /tmp && snomed-ecl-engine use uk > /dev/null && PS1='\n$ ' && clear"
Enter
Show

Type "snomed-ecl-engine expand '< 195967001 |Asthma| : 246075003 |Causative agent| = << 105590001 |Substance|'"
Sleep 600ms
Enter
Sleep 5s

Ctrl+L
Type "snomed-ecl-engine expand '<< 404684003 |Clinical finding|' --count"
Sleep 400ms
Enter
Sleep 2.5s

Type "snomed-ecl-engine expand '<< 195967001 |Asthma|' --csv > asthma.csv && head -5 asthma.csv"
Sleep 400ms
Enter
Sleep 4s
