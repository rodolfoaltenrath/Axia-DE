`dock-icons.json` records the original `LaunchpadIcon` and `TrashIcon` JSX from
`themes/default/components/Dock.jsx` at commit `5ee6da4c`. It contains only widget
data, generated before replacing those functions: 20/54/86 px, both color modes,
and empty/full trash. Repository licensing applies to this reference data.

The Rust test compares native properties and hierarchy against these immutable
references, allowing only floating-point arithmetic tolerance. CSS rotations
are compared to their equivalent native degree properties. No JavaScript
runtime or JSX test source is required to run it.
