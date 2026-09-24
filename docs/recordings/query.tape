Source docs/recordings/settings.tape
Output docs/images/query.gif
Set Width 1440
Set Height 760

Hide
Type "cd /tmp && snomed-ecl-engine use uk > /dev/null && clear"
Enter
Show

Type "snomed-ecl-engine query"
Sleep 400ms
Enter
Sleep 1.5s

Type ":count"
Enter
Sleep 600ms
Type "<< 404684003 |Clinical finding|"
Enter
Sleep 1.2s
Type "<< 64572001 |Disease| : 363698007 |Finding site| = << 39607008 |Lung structure|"
Enter
Sleep 1.2s
Type "<< 64572001 |Disease| : 363698007 |Finding site| = << 39607008 |Lung structure|, 246075003 |Causative agent| = << 49872002 |Virus|"
Enter
Sleep 1.5s
Type ":count"
Enter
Sleep 600ms
Type "(<< 64572001 |Disease| : 363698007 = << 39607008 |Lung structure|, 246075003 = << 49872002 |Virus|) MINUS << 233604007 |Pneumonia|"
Enter
Sleep 5s
