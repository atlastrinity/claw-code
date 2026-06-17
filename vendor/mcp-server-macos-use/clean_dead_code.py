import re

def clean_swift_file():
    path = "Sources/main.swift"
    with open(path, "r") as f:
        lines = f.readlines()

    keep_schemas = {
        "clickSchema", "typeSchema", "refreshSchema", "unifiedVisionSchema", 
        "pressKeySchema", "scrollSchema", "mouseActionSchema", "dragDropSchema", 
        "windowMgmtSchema", "appleScriptSchema", "listWindowsSchema"
    }

    keep_tools = {
        "unifiedVisionTool", "refreshTool", "clickTool", "rightClickTool", 
        "typeTool", "pressKeyTool", "scrollTool", "dragDropTool", 
        "windowMgmtTool", "listWindowsTool", "appleScriptTool", "windowInfoTool"
    }
    
    # We also keep some schemas that might be implicitly used or global, but all specific tool schemas are in keep_schemas.

    new_lines = []
    i = 0
    while i < len(lines):
        line = lines[i]

        # 1. Check Schema
        m = re.match(r'^    let ([A-Za-z0-9_]+Schema): Value = \.object\(\[', line)
        if m:
            schema_name = m.group(1)
            if schema_name not in keep_schemas:
                # skip until `    ])`
                while i < len(lines):
                    if lines[i].startswith("    ])"):
                        i += 1
                        break
                    i += 1
                continue

        # 2. Check Tool
        m2 = re.match(r'^    let ([A-Za-z0-9_]+(?:Tool|AliasTool)) = Tool\(', line)
        if m2:
            tool_name = m2.group(1)
            if tool_name not in keep_tools:
                # skip until `    )`
                while i < len(lines):
                    if lines[i].startswith("    )"):
                        i += 1
                        break
                    i += 1
                continue

        # 3. Check Case in CallTool
        m3 = re.match(r'^            case ([A-Za-z0-9_]+(?:Tool|AliasTool))\.name', line)
        if m3:
            # wait, a case could have multiple tools: `case executeCommandTool.name, terminalTool.name:`
            # Let's extract all tools
            tools_in_case = re.findall(r'([A-Za-z0-9_]+(?:Tool|AliasTool))\.name', line)
            should_remove = True
            for t in tools_in_case:
                if t in keep_tools:
                    should_remove = False
            
            if should_remove:
                # Skip until next case, default:, or the end of the switch `            }`
                i += 1
                while i < len(lines):
                    if re.match(r'^            (case |default:|})', lines[i]):
                        break
                    i += 1
                continue

        new_lines.append(line)
        i += 1

    with open(path, "w") as f:
        f.writelines(new_lines)

clean_swift_file()
