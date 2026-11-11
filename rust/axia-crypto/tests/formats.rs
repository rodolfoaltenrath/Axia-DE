//! Native consumer compatibility. All test code is Rust; the shared library
//! contains the actual vault/backup consumers, not a copied format implementation.
use std::{
    ffi::{c_char, c_void, CStr, CString},
    fs::File,
    os::fd::AsRawFd,
    path::{Path, PathBuf},
    ptr,
};
#[repr(C)]
#[derive(Clone, Copy)]
struct Kdf {
    memory: u32,
    passes: u32,
    lanes: u32,
}
#[repr(C)]
#[derive(Clone, Copy)]
struct Slot {
    kind: u8,
    kdf: u8,
    id: [u8; 8],
    label: [u8; 64],
    p: Kdf,
    salt: [u8; 16],
    cred: [u8; 255],
    cred_len: u8,
    hmac_salt: [u8; 32],
    aaguid: [u8; 16],
    fido_pin: u8,
    created: u64,
    nonce: [u8; 24],
    wrapped: [u8; 32],
    mac: [u8; 16],
}
#[repr(C)]
#[derive(Clone, Copy)]
struct AttLoc {
    id: [u8; 16],
    off: u64,
    len: u64,
    nonce: [u8; 24],
    mac: [u8; 16],
}
#[repr(C)]
struct Header {
    id: [u8; 16],
    created: u64,
    saved: u64,
    counter: u32,
    flags: u16,
    slots: [Slot; 16],
    nslots: i32,
    hlen: usize,
    body_off: u64,
    body_len: u64,
    atts: *mut AttLoc,
    natts: i32,
}
#[repr(C)]
struct Factors {
    password: *const c_char,
    keyfile: *const u8,
    fido: *const u8,
    recovery: *const c_char,
}
#[repr(C)]
struct AttOut {
    id: [u8; 16],
    nonce: [u8; 24],
    mac: [u8; 16],
    cipher: *const u8,
    srcfd: i32,
    off: u64,
    len: u64,
}
type Stream = unsafe extern "C" fn(
    i32,
    i32,
    *const u8,
    Option<unsafe extern "C" fn(*mut c_void, usize) -> i32>,
    *mut c_void,
) -> i32;
struct Api {
    handle: *mut c_void,
    new: unsafe extern "C" fn(*mut Header),
    make: unsafe extern "C" fn(
        *const Header,
        *mut Slot,
        i32,
        *const Factors,
        *const u8,
        *const Kdf,
        *const c_char,
    ) -> i32,
    write: unsafe extern "C" fn(
        i32,
        *mut Header,
        *const u8,
        *const u8,
        usize,
        *const AttOut,
        i32,
    ) -> i32,
    parse: unsafe extern "C" fn(*const u8, usize, *mut Header) -> i32,
    open: unsafe extern "C" fn(*const Header, i32, *const Factors, *mut u8, *mut i32) -> i32,
    body: unsafe extern "C" fn(
        *const u8,
        usize,
        *const Header,
        *const u8,
        *mut *mut u8,
        *mut usize,
    ) -> i32,
    free_header: unsafe extern "C" fn(*mut Header),
    free: unsafe extern "C" fn(*mut c_void),
    keyfile: unsafe extern "C" fn(*const u8, usize, *mut u8),
    seal_att: unsafe extern "C" fn(
        *const u8,
        *const u8,
        *const u8,
        *const u8,
        usize,
        *mut u8,
        *mut u8,
        *mut u8,
    ) -> i32,
    open_att: unsafe extern "C" fn(
        *const u8,
        *const u8,
        *const u8,
        *const u8,
        usize,
        *const u8,
        *const u8,
        *mut u8,
    ) -> i32,
    derive: unsafe extern "C" fn(*const c_char, *const u8, *mut u8) -> i32,
    encrypt: Stream,
    decrypt: Stream,
}
impl Drop for Api {
    fn drop(&mut self) {
        unsafe {
            libc::dlclose(self.handle);
        }
    }
}
impl Api {
    unsafe fn load(path: &Path) -> Self {
        use std::os::unix::ffi::OsStrExt;
        let path = CString::new(path.as_os_str().as_bytes()).unwrap();
        let handle = libc::dlopen(path.as_ptr(), libc::RTLD_NOW | libc::RTLD_LOCAL);
        assert!(
            !handle.is_null(),
            "dlopen: {:?}",
            CStr::from_ptr(libc::dlerror())
        );
        macro_rules! sym {
            ($name:literal,$ty:ty) => {{
                let p = libc::dlsym(handle, concat!($name, "\0").as_ptr().cast());
                assert!(!p.is_null(), "missing {}", $name);
                std::mem::transmute::<*mut c_void, $ty>(p)
            }};
        }
        Self {
            handle,
            new: sym!("vf_header_new", unsafe extern "C" fn(*mut Header)),
            make: sym!(
                "vf_make_slot",
                unsafe extern "C" fn(
                    *const Header,
                    *mut Slot,
                    i32,
                    *const Factors,
                    *const u8,
                    *const Kdf,
                    *const c_char,
                ) -> i32
            ),
            write: sym!(
                "vf_write_fd",
                unsafe extern "C" fn(
                    i32,
                    *mut Header,
                    *const u8,
                    *const u8,
                    usize,
                    *const AttOut,
                    i32,
                ) -> i32
            ),
            parse: sym!(
                "vf_parse",
                unsafe extern "C" fn(*const u8, usize, *mut Header) -> i32
            ),
            open: sym!(
                "vf_open_slot",
                unsafe extern "C" fn(*const Header, i32, *const Factors, *mut u8, *mut i32) -> i32
            ),
            body: sym!(
                "vf_read_body",
                unsafe extern "C" fn(
                    *const u8,
                    usize,
                    *const Header,
                    *const u8,
                    *mut *mut u8,
                    *mut usize,
                ) -> i32
            ),
            free_header: sym!("vf_header_free", unsafe extern "C" fn(*mut Header)),
            free: sym!("sm_free", unsafe extern "C" fn(*mut c_void)),
            keyfile: sym!(
                "vf_keyfile_hash",
                unsafe extern "C" fn(*const u8, usize, *mut u8)
            ),
            seal_att: sym!(
                "vf_att_seal",
                unsafe extern "C" fn(
                    *const u8,
                    *const u8,
                    *const u8,
                    *const u8,
                    usize,
                    *mut u8,
                    *mut u8,
                    *mut u8,
                ) -> i32
            ),
            open_att: sym!(
                "vf_att_open",
                unsafe extern "C" fn(
                    *const u8,
                    *const u8,
                    *const u8,
                    *const u8,
                    usize,
                    *const u8,
                    *const u8,
                    *mut u8,
                ) -> i32
            ),
            derive: sym!(
                "bkc_derive",
                unsafe extern "C" fn(*const c_char, *const u8, *mut u8) -> i32
            ),
            encrypt: sym!("bkc_encrypt_fd", Stream),
            decrypt: sym!("bkc_decrypt_fd", Stream),
        }
    }
}
const PASSWORD: &CStr = c"Axia compatibility fixture - public test password";
const BODY: &[u8] =
    br#"{"entries":[{"name":"compatibility fixture","password":"public test only"}]}"#;
const ATTACHMENT: &[u8] = b"legacy attachment with known test content";
const KEY: [u8; 32] = [0x42; 32];
const SALT: [u8; 16] = [0x33; 16];
fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}
fn library() -> Api {
    unsafe {
        Api::load(&PathBuf::from(
            std::env::var_os("AXIA_CRYPTO_FORMAT_LIBRARY")
                .expect("build the native format library with make test-crypto-native"),
        ))
    }
}
fn backup_content() -> Vec<u8> {
    (0..65537).map(|i| (i % 251) as u8).collect()
}
unsafe fn write_vault(api: &Api, path: &Path) {
    let mut h: Header = std::mem::zeroed();
    (api.new)(&mut h);
    let mut kf = [0; 32];
    (api.keyfile)([0x24; 64].as_ptr(), 64, kf.as_mut_ptr());
    let f = Factors {
        password: PASSWORD.as_ptr(),
        keyfile: kf.as_ptr(),
        fido: ptr::null(),
        recovery: ptr::null(),
    };
    let kdf = Kdf {
        memory: 32,
        passes: 2,
        lanes: 2,
    };
    for (i, kind) in [1, 2].into_iter().enumerate() {
        let slot = ptr::addr_of_mut!(h.slots).cast::<Slot>().add(i);
        assert_eq!(
            (api.make)(
                &h,
                slot,
                kind,
                &f,
                KEY.as_ptr(),
                &kdf,
                c"compatibility".as_ptr()
            ),
            0
        );
        h.nslots += 1;
    }
    let mut cipher = vec![0; ATTACHMENT.len()];
    let mut nonce = [0; 24];
    let mut mac = [0; 16];
    let id = [0x17; 16];
    assert_eq!(
        (api.seal_att)(
            KEY.as_ptr(),
            h.id.as_ptr(),
            id.as_ptr(),
            ATTACHMENT.as_ptr(),
            ATTACHMENT.len(),
            cipher.as_mut_ptr(),
            nonce.as_mut_ptr(),
            mac.as_mut_ptr()
        ),
        0
    );
    let attachment = AttOut {
        id,
        nonce,
        mac,
        cipher: cipher.as_ptr(),
        srcfd: -1,
        off: 0,
        len: cipher.len() as u64,
    };
    let file = File::create(path).unwrap();
    assert_eq!(
        (api.write)(
            file.as_raw_fd(),
            &mut h,
            KEY.as_ptr(),
            BODY.as_ptr(),
            BODY.len(),
            &attachment,
            1
        ),
        0
    );
    (api.free_header)(&mut h);
}
unsafe fn read_vault(api: &Api, path: &Path) {
    let data = std::fs::read(path).unwrap();
    let mut h: Header = std::mem::zeroed();
    assert_eq!((api.parse)(data.as_ptr(), data.len(), &mut h), 0);
    assert_eq!((h.nslots, h.natts), (2, 1));
    let mut kf = [0; 32];
    (api.keyfile)([0x24; 64].as_ptr(), 64, kf.as_mut_ptr());
    for kind in [1, 2] {
        let f = Factors {
            password: if kind == 1 {
                PASSWORD.as_ptr()
            } else {
                ptr::null()
            },
            keyfile: if kind == 2 { kf.as_ptr() } else { ptr::null() },
            fido: ptr::null(),
            recovery: ptr::null(),
        };
        let mut vk = [0; 32];
        let mut slot = -1;
        assert_eq!((api.open)(&h, kind, &f, vk.as_mut_ptr(), &mut slot), 0);
        assert_eq!(vk, KEY);
        let mut plain = ptr::null_mut();
        let mut len = 0;
        assert_eq!(
            (api.body)(
                data.as_ptr(),
                data.len(),
                &h,
                vk.as_ptr(),
                &mut plain,
                &mut len
            ),
            0
        );
        assert_eq!(std::slice::from_raw_parts(plain, len), BODY);
        (api.free)(plain.cast());
    }
    let wrong = Factors {
        password: c"wrong password".as_ptr(),
        keyfile: ptr::null(),
        fido: ptr::null(),
        recovery: ptr::null(),
    };
    let mut vk = [0; 32];
    let mut slot = -1;
    assert_eq!((api.open)(&h, 1, &wrong, vk.as_mut_ptr(), &mut slot), -5);
    let a = &*h.atts;
    let mut plain = vec![0; a.len as usize];
    assert_eq!(
        (api.open_att)(
            KEY.as_ptr(),
            h.id.as_ptr(),
            a.id.as_ptr(),
            data[a.off as usize..].as_ptr(),
            a.len as usize,
            a.nonce.as_ptr(),
            a.mac.as_ptr(),
            plain.as_mut_ptr()
        ),
        0
    );
    assert_eq!(plain, ATTACHMENT);
    (api.free_header)(&mut h);
}
unsafe fn write_backup(api: &Api, path: &Path, plain_path: &Path) {
    let content = backup_content();
    std::fs::write(plain_path, &content).unwrap();
    let input = File::open(plain_path).unwrap();
    let out = File::create(path).unwrap();
    let mut key = [0; 32];
    assert_eq!(
        (api.derive)(PASSWORD.as_ptr(), SALT.as_ptr(), key.as_mut_ptr()),
        0
    );
    assert_eq!(
        (api.encrypt)(
            input.as_raw_fd(),
            out.as_raw_fd(),
            key.as_ptr(),
            None,
            ptr::null_mut()
        ),
        0
    );
}
unsafe fn read_backup(api: &Api, path: &Path, out_path: &Path) {
    let mut key = [0; 32];
    assert_eq!(
        (api.derive)(PASSWORD.as_ptr(), SALT.as_ptr(), key.as_mut_ptr()),
        0
    );
    let input = File::open(path).unwrap();
    let out = File::create(out_path).unwrap();
    assert_eq!(
        (api.decrypt)(
            input.as_raw_fd(),
            out.as_raw_fd(),
            key.as_ptr(),
            None,
            ptr::null_mut()
        ),
        0
    );
    assert_eq!(std::fs::read(out_path).unwrap(), backup_content());
}
#[test]
#[ignore = "explicit generation using the old backend only"]
fn record_legacy_files() {
    assert_eq!(std::env::var("AXIA_RECORD_CRYPTO_FIXTURES").unwrap(), "yes");
    let api = library();
    let folder = fixtures();
    unsafe {
        write_vault(&api, &folder.join("legacy.lvault"));
        read_vault(&api, &folder.join("legacy.lvault"));
        write_backup(
            &api,
            &folder.join("legacy.lbk"),
            &folder.join("temporary-plain.bin"),
        );
    }
    std::fs::remove_file(folder.join("temporary-plain.bin")).unwrap();
}
#[test]
#[ignore = "requires actual native format consumers: make test-crypto-native"]
fn existing_vault_backup_slots_and_attachment() {
    let api = library();
    let temp = std::env::temp_dir().join(format!("axia-crypto-existing-{}", std::process::id()));
    unsafe {
        read_vault(&api, &fixtures().join("legacy.lvault"));
        read_backup(&api, &fixtures().join("legacy.lbk"), &temp);
    }
    std::fs::remove_file(temp).unwrap();
}
#[test]
#[ignore = "requires both native backends for a bidirectional migration check"]
fn old_backend_reads_new_files() {
    let api = library();
    let old_path = std::env::var_os("AXIA_LEGACY_CRYPTO_FORMAT_LIBRARY")
        .expect("provide the preserved old library");
    let old = unsafe { Api::load(&PathBuf::from(old_path)) };
    let temp = std::env::temp_dir().join(format!("axia-crypto-reverse-{}", std::process::id()));
    std::fs::create_dir_all(&temp).unwrap();
    unsafe {
        write_vault(&api, &temp.join("new.lvault"));
        read_vault(&old, &temp.join("new.lvault"));
        write_backup(&api, &temp.join("new.lbk"), &temp.join("plain"));
        read_backup(&old, &temp.join("new.lbk"), &temp.join("decoded"));
    }
    std::fs::remove_dir_all(temp).unwrap();
}
