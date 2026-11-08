// Compatibility entry point: native pickers share the Rust Arquivos application.
import { createElement as h } from 'react';
import { RustFileDialog } from './rustfiles.js';
export function FilePicker(props) {
  return h(RustFileDialog, { ...props, mode: props.mode || (props.save ? 'save' : props.directory ? 'folder' : 'open'), name: props.name ?? props.suggest ?? '' });
}
