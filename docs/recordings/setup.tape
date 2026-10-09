Source docs/recordings/settings.tape
Output docs/images/setup.gif
Set Width 1440
Set Height 760

# scripts/record.sh puts TRUD_API_KEY in place of the marker below, in a
# temporary copy; the prompt hides what is typed. The library starts with one older UK index.
Hide
Type "cd /tmp && snomed-ecl-engine use uk > /dev/null && clear"
Enter
Show

Type "snomed-ecl-engine login"
Sleep 400ms
Enter
# Never type the key unless the hidden prompt itself is showing; on timeout
# VHS stops before typing anything.
Wait+Screen@15s /TRUD API key ›/
Sleep 1s
Type@5ms "@TRUD_KEY@"
Sleep 600ms
Enter
Sleep 3s

Type "snomed-ecl-engine download"
Sleep 400ms
Enter
Sleep 1.5s
Down
Sleep 600ms
Up
Sleep 600ms
Enter
Wait+Screen@60s /Release/
Sleep 1.5s
Down
Sleep 500ms
Down
Sleep 500ms
Up
Sleep 500ms
Up
Sleep 800ms
Enter
Sleep 6s

# Building takes minutes; the clip resumes when the index is selected.
Hide
Wait+Screen@30m /It is now selected/
Show
Sleep 3s

Type "clear"
Enter
Type "snomed-ecl-engine use"
Sleep 400ms
Enter
Sleep 1.5s
Down
Sleep 800ms
Enter
Sleep 2.5s

Type "clear"
Enter
Type "snomed-ecl-engine expand '<< 195967001 |Asthma|' --count"
Sleep 400ms
Enter
Sleep 4s
