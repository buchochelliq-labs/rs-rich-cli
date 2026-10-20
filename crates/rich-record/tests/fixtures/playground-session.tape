# A session from the rs-rich docs playground, 2026-10-20.
# Replay it with `rich record session.tape` (rs-rich-cli, a native build).
Set Title "rich playground session"
Set Size 100x28
Set TypingDelay 40ms
Write data.json "{\\n  \"name\": \"rs-rich\",\\n  \"version\": \"0.0.21\",\\n  \"license\": \"MIT\",\\n  \"keywords\": [\\n    \"terminal\",\\n    \"ansi\",\\n    \"tables\",\\n    \"markdown\"\\n  ],\\n  \"stable\": false,\\n  \"downloads\": 120431,\\n  \"homepage\": null,\\n  \"authors\": [\\n    {\\n      \"name\": \"Ada\",\\n      \"email\": \"ada@example.com\"\\n    },\\n    {\\n      \"name\": \"Grace\",\\n      \"email\": \"grace@example.com\"\\n    }\\n  ],\\n  \"features\": {\\n    \"default\": [\\n      \"syntax\",\\n      \"markdown\"\\n    ],\\n    \"art\": [\\n      \"image\",\\n      \"gif\"\\n    ]\\n  }\\n}\\n"
# photo.png is a sample from the playground; put a copy beside this tape.
Write tricky.txt "say \"hi\"\\\\n\\tnot a newline: \\\\ end\\n"
Type "rich data.json"
Enter
Sleep 1500ms
Type "echo \"# A \\\"quoted\\\" title\" | rich - --markdown"
Enter
Sleep 3000ms
Type "rich photo.png --width 20"
Enter
Sleep 1500ms
Type "cat tricky.txt"
Enter
Sleep 2000ms
