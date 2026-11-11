//! Mail format parsers for axia-maild, exported with the C ABI of the
//! former `server/src/mail/{mime,xml,html,ical,vcard}.c`.
//!
//! The headers in `server/src/mail/*.h` remain the contract: every exported
//! struct is `#[repr(C)]` with the same fields, and every exported function
//! keeps its name, arguments and ownership. Returned buffers come from
//! `malloc` and C releases them with `free()` (or the matching `*_free`).
//! Trees handed to C (`MxNode`, `IcComp`) are released only by `mx_free` and
//! `ic_free`.
//!
//! Untrusted input never panics: indexing is checked and the C quirks that
//! callers rely on (truncation sizes, C-string semantics) are preserved.
//! Time zones are resolved by the C TZif reader (`server/src/ui/tzif.c`),
//! which axia-maild links.

mod c;
mod charset_tables;
mod html;
mod html_entities;
mod ical;
mod jbuf;
mod mime;
mod vcard;
mod xml;

pub use html::{HtmlData, HtmlStats};
pub use ical::{IcAttendee, IcBuild, IcComp, IcEvent, IcProp};
pub use jbuf::JBuf;
pub use mime::{MimeAddr, MimeMsg, MimePart};
pub use vcard::JVal;
pub use xml::MxNode;

/// Sizes and field offsets of the exported structs, so the C test can check
/// them against the headers (`tests/mail_test.c`).
#[unsafe(no_mangle)]
pub extern "C" fn axia_mail_layout(which: core::ffi::c_int) -> usize {
    use core::mem::{offset_of, size_of};
    match which {
        0 => size_of::<MimePart>(),
        1 => size_of::<MimeMsg>(),
        2 => size_of::<MimeAddr>(),
        3 => size_of::<MxNode>(),
        4 => size_of::<HtmlStats>(),
        5 => size_of::<HtmlData>(),
        6 => size_of::<IcProp>(),
        7 => size_of::<IcComp>(),
        8 => size_of::<IcAttendee>(),
        9 => size_of::<IcEvent>(),
        10 => size_of::<IcBuild>(),
        11 => size_of::<JBuf>(),
        12 => size_of::<JVal>(),
        13 => offset_of!(MimePart, hdr),
        14 => offset_of!(IcEvent, description),
        15 => offset_of!(IcEvent, att),
        16 => offset_of!(IcEvent, parent_uid),
        17 => offset_of!(IcBuild, exdate),
        18 => offset_of!(MxNode, tlen),
        19 => offset_of!(IcComp, kid),
        _ => 0,
    }
}

#[cfg(test)]
mod tests;
