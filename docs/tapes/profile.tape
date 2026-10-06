# rich profile: what each column of a CSV file holds (types, nulls,
# statistics, distributions and a map of the nulls), and `--csv --infer`
# showing the inferred types on the table itself. The file is the fixture
# docs/tapes/fixtures/data/orders.csv.
Set Title "rich profile · what a data file holds"
Set Size 100x50
Set TypingDelay 40ms
Hide
Exec "cp $REPO/docs/tapes/fixtures/data/orders.csv ."
Type "clear"
Enter
Wait /❯\s*$/
Show
Type "rich profile orders.csv --columns region,amount,express --top 3"
Enter
Wait "0 [ ░▒▓█] 100"
Sleep 1500ms
Screenshot profile
Hide
Type "clear"
Enter
Show
Type "rich orders.csv --infer --head 8"
Enter
Wait /└─+┴/
Sleep 1500ms
Screenshot infer
