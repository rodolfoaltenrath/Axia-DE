//! BPF has no runtime. The ELF packer rejects any unresolved intrinsic; all
//! integer and memory operations must be lowered inline by the BPF backend.
#![feature(no_core, compiler_builtins)]
#![allow(internal_features)]
#![no_core]
#![compiler_builtins]
