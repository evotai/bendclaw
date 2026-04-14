---
name: tmux-tui-testing
description: Test TUI (Text User Interface) applications using tmux. Use this skill when you need to automate testing of terminal-based applications by sending keystrokes and capturing pane output.
---

<objective>
Automate testing of TUI applications by controlling them through tmux sessions. This enables programmatic interaction with terminal apps that render visual interfaces, allowing you to send input, wait for rendering, and assert on captured output.
</objective>

<essential_principles>
## Why tmux for TUI Testing

TUI applications render to a terminal rather than producing structured output. tmux provides:
- Detached sessions for headless testing
- Precise window/pane sizing for consistent layouts
- Keystroke injection via `send-keys`
- Pane content capture with `capture-pane`
- ANSI sequence preservation for color/style assertions

## Critical Timing Considerations

TUI apps need time to:
1. Start up and initialize
2. Render after receiving input
3. Complete async operations

Always add appropriate waits between actions. Start with 0.5-1s delays and adjust based on app behavior.
</essential_principles>

<core_commands>
## Session Management

```bash
# Create detached session
tmux new-session -d -s "$SESSION_NAME"

# Create named window in session
tmux new-window -t "$SESSION_NAME" -n "$WINDOW_NAME"

# Force specific dimensions (critical for consistent TUI rendering)
tmux resize-window -t "$SESSION_NAME:$WINDOW_NAME" -x $WIDTH -y $HEIGHT

# Kill session when done
tmux kill-session -t "$SESSION_NAME"
```

## Sending Input

```bash
# Send command + Enter
tmux send-keys -t "$SESSION_NAME:$WINDOW_NAME" "your-command" Enter

# Send special keys
tmux send-keys -t "$SESSION_NAME:$WINDOW_NAME" Up        # Arrow up
tmux send-keys -t "$SESSION_NAME:$WINDOW_NAME" Down      # Arrow down
tmux send-keys -t "$SESSION_NAME:$WINDOW_NAME" Tab       # Tab
tmux send-keys -t "$SESSION_NAME:$WINDOW_NAME" Escape    # Escape
tmux send-keys -t "$SESSION_NAME:$WINDOW_NAME" C-c       # Ctrl+C
tmux send-keys -t "$SESSION_NAME:$WINDOW_NAME" C-d       # Ctrl+D
tmux send-keys -t "$SESSION_NAME:$WINDOW_NAME" Space     # Space
tmux send-keys -t "$SESSION_NAME:$WINDOW_NAME" BSpace    # Backspace

# Send literal text (no special key interpretation)
tmux send-keys -t "$SESSION_NAME:$WINDOW_NAME" -l "literal text"
```

## Capturing Output

```bash
# Capture visible pane content (plain text)
tmux capture-pane -t "$SESSION_NAME:$WINDOW_NAME" -p

# Capture with ANSI escape sequences (colors, styles)
tmux capture-pane -t "$SESSION_NAME:$WINDOW_NAME" -p -e

# Capture including scrollback history
tmux capture-pane -t "$SESSION_NAME:$WINDOW_NAME" -p -S -1000
```
</core_commands>

<process>
## Testing Workflow

1. **Setup**: Create detached tmux session with fixed dimensions
2. **Launch**: Start the TUI application in the session
3. **Wait**: Allow time for app to initialize and render
4. **Interact**: Send keystrokes to navigate/interact
5. **Wait**: Allow time for UI to update after each interaction
6. **Capture**: Get pane contents
7. **Assert**: Check captured output for expected content
8. **Cleanup**: Kill the tmux session

## Example Test Sequence

```bash
SESSION="tui-test-$$"
WIDTH=80
HEIGHT=24

# Setup
tmux new-session -d -s "$SESSION"
tmux resize-window -t "$SESSION" -x $WIDTH -y $HEIGHT

# Launch app
tmux send-keys -t "$SESSION" "./my-tui-app" Enter
sleep 1  # Wait for startup

# Interact
tmux send-keys -t "$SESSION" Down Down Enter
sleep 0.5  # Wait for render

# Capture and check
OUTPUT=$(tmux capture-pane -t "$SESSION" -p)
if echo "$OUTPUT" | grep -q "Expected Text"; then
    echo "PASS: Found expected content"
else
    echo "FAIL: Expected content not found"
    echo "Captured output:"
    echo "$OUTPUT"
fi

# Cleanup
tmux kill-session -t "$SESSION"
```
</process>

<common_patterns>
## Pattern: Wait for Specific Text

```bash
wait_for_text() {
    local session="$1"
    local text="$2"
    local timeout="${3:-10}"
    local elapsed=0

    while [ $elapsed -lt $timeout ]; do
        if tmux capture-pane -t "$session" -p | grep -q "$text"; then
            return 0
        fi
        sleep 0.2
        elapsed=$((elapsed + 1))
    done
    return 1
}

# Usage
if wait_for_text "$SESSION" "Ready"; then
    echo "App is ready"
fi
```

## Pattern: Snapshot Comparison

```bash
# Capture baseline
tmux capture-pane -t "$SESSION" -p > expected.txt

# Later, compare
tmux capture-pane -t "$SESSION" -p > actual.txt
diff expected.txt actual.txt
```

## Pattern: Test Multiple Scenarios

```bash
run_test() {
    local name="$1"
    local keys="$2"
    local expected="$3"

    # Reset to known state
    tmux send-keys -t "$SESSION" C-c
    sleep 0.3
    tmux send-keys -t "$SESSION" "./app" Enter
    sleep 1

    # Execute test
    tmux send-keys -t "$SESSION" $keys
    sleep 0.5

    # Verify
    if tmux capture-pane -t "$SESSION" -p | grep -q "$expected"; then
        echo "PASS: $name"
    else
        echo "FAIL: $name"
    fi
}

run_test "Navigate down" "Down Down" "Item 3"
run_test "Select item" "Enter" "Selected:"
```
</common_patterns>

<advanced_patterns>
## Mouse Events

```bash
# Enable mouse support in tmux (add to session or tmux.conf)
tmux set-option -t "$SESSION" mouse on

# Click at specific coordinates (x, y from top-left)
# Uses escape sequence: \e[<button;x;y;M for press, m for release
send_mouse_click() {
    local session="$1"
    local x="$2"
    local y="$3"

    # Send mouse press at (x, y) - button 0 = left click
    printf '\e[<0;%d;%dM' "$x" "$y" | tmux load-buffer -
    tmux paste-buffer -t "$session"

    # Send mouse release
    printf '\e[<0;%d;%dm' "$x" "$y" | tmux load-buffer -
    tmux paste-buffer -t "$session"
}

# Click at row 5, column 10
send_mouse_click "$SESSION" 10 5

# Alternative: Use send-keys with MouseDown/MouseUp (tmux 3.0+)
tmux send-keys -t "$SESSION" -M MouseDown1Pane 10 5
tmux send-keys -t "$SESSION" -M MouseUp1Pane 10 5
```

## Clipboard Operations

```bash
# Copy text TO tmux buffer (for pasting into app)
echo "text to paste" | tmux load-buffer -

# Paste from tmux buffer into pane
tmux paste-buffer -t "$SESSION"

# Set buffer directly
tmux set-buffer "content to paste"

# Capture pane to buffer, then retrieve
tmux capture-pane -t "$SESSION" -b capture-buffer
tmux show-buffer -b capture-buffer

# For apps that use system clipboard, set DISPLAY for xclip/xsel
# or use pbcopy/pbpaste on macOS
tmux send-keys -t "$SESSION" "echo 'copied' | pbcopy" Enter
```

## Multiple Panes

```bash
# Split window horizontally (top/bottom)
tmux split-window -t "$SESSION" -v

# Split window vertically (left/right)
tmux split-window -t "$SESSION" -h

# Address specific panes (0-indexed)
tmux send-keys -t "$SESSION:0.0" "left pane" Enter   # First pane
tmux send-keys -t "$SESSION:0.1" "right pane" Enter  # Second pane

# Resize panes
tmux resize-pane -t "$SESSION:0.0" -x 40  # Set width
tmux resize-pane -t "$SESSION:0.1" -y 10  # Set height

# Capture specific pane
tmux capture-pane -t "$SESSION:0.1" -p

# Select/focus a pane
tmux select-pane -t "$SESSION:0.1"

# List panes with their indices and dimensions
tmux list-panes -t "$SESSION" -F "#{pane_index}: #{pane_width}x#{pane_height}"
```

## Pattern: Testing Split-Pane Apps

```bash
# Some TUIs use split layouts - test each pane independently
setup_split_test() {
    tmux new-session -d -s "$SESSION"
    tmux resize-window -t "$SESSION" -x 120 -y 40

    # Launch app that creates splits
    tmux send-keys -t "$SESSION" "./split-app" Enter
    sleep 1
}

verify_pane_content() {
    local pane="$1"
    local expected="$2"

    local content
    content=$(tmux capture-pane -t "$SESSION:0.$pane" -p)

    if echo "$content" | grep -q "$expected"; then
        echo "PASS: Pane $pane contains '$expected'"
        return 0
    else
        echo "FAIL: Pane $pane missing '$expected'"
        return 1
    fi
}

# Test left pane shows file list, right pane shows preview
verify_pane_content 0 "file1.txt"
verify_pane_content 1 "Preview:"
```

## Pattern: Scrolling and Viewport

```bash
# Scroll up/down in pane (for apps with scrolling content)
tmux send-keys -t "$SESSION" PageUp
tmux send-keys -t "$SESSION" PageDown

# Enter copy mode to scroll (then exit)
tmux copy-mode -t "$SESSION"
tmux send-keys -t "$SESSION" -X scroll-up
tmux send-keys -t "$SESSION" -X scroll-down
tmux send-keys -t "$SESSION" q  # Exit copy mode

# Capture with scrollback (negative = lines above visible)
tmux capture-pane -t "$SESSION" -p -S -100 -E -1  # Last 100 lines of scrollback

# Capture everything (visible + all scrollback)
tmux capture-pane -t "$SESSION" -p -S - -E -
```

## Pattern: Environment and Shell Setup

```bash
# Set environment variables before launching app
tmux send-keys -t "$SESSION" "export TERM=xterm-256color" Enter
tmux send-keys -t "$SESSION" "export NO_COLOR=1" Enter  # Disable colors for easier parsing
tmux send-keys -t "$SESSION" "export LC_ALL=en_US.UTF-8" Enter

# Start with clean shell
tmux send-keys -t "$SESSION" "env -i bash --norc --noprofile" Enter
tmux send-keys -t "$SESSION" "export TERM=xterm-256color" Enter

# Change directory
tmux send-keys -t "$SESSION" "cd /path/to/project" Enter
```

## Pattern: Waiting for App Exit

```bash
wait_for_exit() {
    local session="$1"
    local timeout="${2:-30}"
    local elapsed=0

    while [ $elapsed -lt $timeout ]; do
        # Check if pane is showing shell prompt (app exited)
        if tmux capture-pane -t "$session" -p | grep -qE '^\$|^>|^%'; then
            return 0
        fi
        sleep 0.5
        elapsed=$((elapsed + 1))
    done
    return 1
}

# Run app and wait for it to finish
tmux send-keys -t "$SESSION" "./app --batch-mode" Enter
if wait_for_exit "$SESSION" 60; then
    echo "App completed"
else
    echo "App timed out"
    tmux send-keys -t "$SESSION" C-c  # Force quit
fi
```

## Pattern: Capture with Line Numbers

```bash
# Useful for debugging - see exactly which line contains what
capture_with_lines() {
    local session="$1"
    tmux capture-pane -t "$session" -p | nl -ba
}

# Assert content at specific line
assert_line() {
    local session="$1"
    local line_num="$2"
    local expected="$3"

    local actual
    actual=$(tmux capture-pane -t "$session" -p | sed -n "${line_num}p")

    if [ "$actual" = "$expected" ]; then
        echo "PASS: Line $line_num matches"
        return 0
    else
        echo "FAIL: Line $line_num"
        echo "  Expected: '$expected'"
        echo "  Actual:   '$actual'"
        return 1
    fi
}

# Check that line 3 shows the header
assert_line "$SESSION" 3 "=== My App v1.0 ==="
```
</advanced_patterns>

<troubleshooting>
## Common Issues

**Problem**: Captured output is empty or incomplete
- **Cause**: App hasn't rendered yet
- **Fix**: Increase sleep duration after send-keys

**Problem**: Layout looks wrong in captures
- **Cause**: Window dimensions don't match app expectations
- **Fix**: Use `resize-window -x WIDTH -y HEIGHT` before launching app

**Problem**: Special characters not sent correctly
- **Cause**: Using wrong key names
- **Fix**: Check `man tmux` for correct key names (e.g., `BSpace` not `Backspace`)

**Problem**: ANSI codes making assertions difficult
- **Cause**: Using `-e` flag when not needed
- **Fix**: Omit `-e` for plain text, or strip ANSI with `sed 's/\x1b\[[0-9;]*m//g'`

**Problem**: Tests flaky/inconsistent
- **Cause**: Race conditions with async rendering
- **Fix**: Use `wait_for_text` pattern instead of fixed sleeps
</troubleshooting>

<success_criteria>
A successful TUI test:
- Runs in a detached tmux session (no display required)
- Uses fixed dimensions for reproducible layouts
- Waits appropriately for rendering
- Captures pane content and asserts on expected text
- Cleans up the session after completion
- Handles both success and failure cases gracefully
</success_criteria>
