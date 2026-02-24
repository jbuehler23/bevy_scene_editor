<img width="1917" height="1033" alt="image" src="https://github.com/user-attachments/assets/fa53542b-de0a-420b-bde0-bdcb180992a5" />

```
cargo run --example basic
```

## Keyboard Shortcuts

> **Note:** This project is a work in progress. Not all keybindings listed below are fully implemented yet.

### Navigation

| Key | Action |
|-----|--------|
| MMB | Orbit |
| Shift+MMB | Pan |
| Scroll | Zoom |
| F | Focus selected |
| Shift+F | Walk mode |
| Numpad . | Orbit to selection |
| Shift+MMB click | Set orbit center |
| Ctrl+1-9 | Save camera bookmark |
| 1-9 | Restore camera bookmark |

### Walk Mode

| Key | Action |
|-----|--------|
| W / A / S / D | Move forward / left / back / right |
| Q / E | Move down / up |
| Shift | Double speed |
| Scroll | Adjust speed |
| Click / Enter | Confirm position |
| Esc / Right-click | Cancel (restore camera) |

### Transform

| Key | Action |
|-----|--------|
| W | Translate mode |
| E | Rotate mode |
| R | Scale mode |
| X | Toggle local / world space |
| . (Period) | Toggle snap |
| G | Grab (modal translate) |
| S | Scale (modal) |
| R | Rotate (modal) |
| X / Y / Z | Axis constraint |
| Shift+X / Y / Z | Plane constraint (exclude axis) |
| Click / Enter | Confirm |
| Esc / Right-click | Cancel |
| Ctrl (during drag) | Toggle snap |

### Entity

| Key | Action |
|-----|--------|
| Delete / Backspace | Delete selected |
| Ctrl+D | Duplicate |
| Ctrl+C | Copy components |
| Ctrl+V | Paste components |
| H | Toggle visibility |
| Alt+G | Reset position |
| Alt+R | Reset rotation |
| Alt+S | Reset scale |

### Brush Editing

| Key | Action |
|-----|--------|
| ` (Backtick) | Enter / exit brush edit |
| 1 | Vertex mode |
| 2 | Edge mode |
| 3 | Face mode |
| 4 | Clip mode |
| G / E | Grab / Extrude |
| X / Y / Z | Constrain axis |
| Ctrl+Click | Multi-select |
| Delete | Delete selected element |
| Enter | Apply clip plane |
| Esc | Cancel / Clear |

### Brush Draw

| Key | Action |
|-----|--------|
| B | Activate draw mode |
| Tab | Toggle Add / Cut mode |
| Click | Advance drawing phase |
| Esc / Right-click | Cancel drawing |

### View

| Key | Action |
|-----|--------|
| Ctrl+Shift+W | Toggle wireframe |
| [ | Decrease grid size |
| ] | Increase grid size |

### File

| Key | Action |
|-----|--------|
| Ctrl+S | Save scene |
| Ctrl+O | Open scene |
| Ctrl+Shift+N | New scene |
| Ctrl+Z | Undo |
| Ctrl+Shift+Z | Redo |
