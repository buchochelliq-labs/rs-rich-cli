# rich --watch re-renders a file every time it is saved.
Set Title "rich --watch · re-render on save"
Set Size 90x22
Write service.json '{"service": "checkout", "replicas": 2, "healthy": true, "regions": ["eu-west-1", "us-east-1"]}'
Type "rich --watch service.json"
Sleep 300ms
Enter
Wait /"replicas": 2/
Sleep 1s
Screenshot before
Exec "sed -i 's/\"replicas\": 2/\"replicas\": 5/' service.json"
Wait /"replicas": 5/
Sleep 1s
Screenshot after
Ctrl+C
Wait /❯\s*$/
