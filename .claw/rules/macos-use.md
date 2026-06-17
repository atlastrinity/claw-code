# Instructions for using macos-use tools

When the user asks to interact with the macOS interface (click a button, type text into a field, select an element, etc.):

1. **NEVER guess coordinates** based on screen proportions (e.g., 960x1080 for bottom center).
2. **ALWAYS use the `macos-use_vision` tool first** to scan the screen or a specific window. This will provide you with the exact OCR coordinates of the elements (e.g., input fields, buttons, labels).
3. After obtaining the exact coordinates from `macos-use_vision`, use them for tools like `macos-use_click_and_traverse` or `macos-use_type_and_traverse`.
4. If you are not sure which window is currently in the foreground, first use `macos-use_list_all_windows` and make the required window active using `macos-use_window_management` (action `make_front`).
