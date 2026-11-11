`dock-layout.json` captures geometry computed by the original Dock JSX at
`bb6c326d`, before its geometry was replaced. The 96 cases cover bottom/left/right
placement, offset displays, narrow screens, RTL, pinned/recent items, dragging,
placeholders and magnification. Repository licensing applies.

The Rust test compares the entire result to these immutable observations, with
floating-point tolerance only. JavaScript is not needed to run the regression.
