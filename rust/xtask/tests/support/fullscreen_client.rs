//! A minimal real xdg-shell/shm client for the switcher compositor contract.
//! No generated protocol bindings, GPU, shell command or active desktop is needed.
use std::{fs, io::{Read, Write}, os::{fd::{AsRawFd,FromRawFd}, unix::net::UnixStream}, path::Path, time::{Duration,Instant}};
fn words(values: &[u32]) -> Vec<u8> { values.iter().flat_map(|value|value.to_ne_bytes()).collect() }
fn string(value: &str) -> Vec<u8> {
    let mut out = words(&[value.len() as u32+1]);
    out.extend_from_slice(value.as_bytes());out.push(0);
    while out.len()%4 != 0 { out.push(0); }
    out
}
fn send(socket: &mut UnixStream, object: u32, opcode: u16, args: &[u8]) -> Result<(),String> {
    let mut message = words(&[object,((args.len() as u32+8)<<16)|u32::from(opcode)]);
    message.extend_from_slice(args);socket.write_all(&message).map_err(|e|e.to_string())
}
fn event(socket: &mut UnixStream) -> Result<(u32,u16,Vec<u8>),String> {
    let mut header=[0u8;8];socket.read_exact(&mut header).map_err(|e|e.to_string())?;
    let word=|offset|u32::from_ne_bytes(header[offset..offset+4].try_into().unwrap());
    let size=(word(4)>>16) as usize;
    if !(8..=65535).contains(&size) || size%4 != 0 {return Err("invalid Wayland event length".into());}
    let mut data=vec![0;size-8];socket.read_exact(&mut data).map_err(|e|e.to_string())?;
    if word(0)==1 && word(4) as u16==0 {return Err(format!("Wayland protocol error: {data:?}"));}
    Ok((word(0),word(4) as u16,data))
}
fn number(data:&[u8],offset:usize)->u32 {u32::from_ne_bytes(data[offset..offset+4].try_into().unwrap())}
fn bind(socket:&mut UnixStream,name:u32,interface:&str,id:u32)->Result<(),String> {
    let mut args=words(&[name]);args.extend(string(interface));args.extend(words(&[1,id]));send(socket,2,0,&args)
}
fn pool(socket:&mut UnixStream,fd:i32,size:usize)->Result<(),String> {
    let mut message=words(&[5,(16<<16),10,size as u32]);
    let mut iov=libc::iovec{iov_base:message.as_mut_ptr().cast(),iov_len:message.len()};
    // usize storage keeps the ancillary header aligned.
    let mut control=[0usize;8];
    let mut header:libc::msghdr=unsafe{std::mem::zeroed()};
    header.msg_iov=&mut iov;header.msg_iovlen=1;header.msg_control=control.as_mut_ptr().cast();
    header.msg_controllen=unsafe{libc::CMSG_SPACE(std::mem::size_of::<i32>() as u32)} as usize;
    unsafe {
        let cmsg=libc::CMSG_FIRSTHDR(&header);
        (*cmsg).cmsg_level=libc::SOL_SOCKET;(*cmsg).cmsg_type=libc::SCM_RIGHTS;
        (*cmsg).cmsg_len=libc::CMSG_LEN(std::mem::size_of::<i32>() as u32) as usize;
        std::ptr::write(libc::CMSG_DATA(cmsg).cast::<i32>(),fd);
        let sent=libc::sendmsg(socket.as_raw_fd(),&header,libc::MSG_NOSIGNAL);
        if sent<0 {return Err(std::io::Error::last_os_error().to_string());}
        if sent as usize != message.len() {return Err("short Wayland shm request".into());}
    }
    Ok(())
}
pub fn run(home:&Path)->Result<(),String> {
    let until=Instant::now()+Duration::from_secs(24);
    while !fs::read_to_string(home.join(".cache/axia/session.log")).unwrap_or_default().contains("switcher-client-start") {
        if Instant::now()>=until{return Err("fullscreen fixture was not requested".into());}
        std::thread::sleep(Duration::from_millis(10));
    }
    let runtime=home.join("runtime");
    let path=fs::read_dir(&runtime).map_err(|e|e.to_string())?.filter_map(Result::ok)
        .find(|entry|entry.file_name().to_string_lossy().starts_with("wayland-") && !entry.file_name().to_string_lossy().ends_with(".lock"))
        .ok_or("Wayland socket is missing")?.path();
    let mut socket=UnixStream::connect(path).map_err(|e|e.to_string())?;
    socket.set_read_timeout(Some(Duration::from_secs(6))).map_err(|e|e.to_string())?;
    send(&mut socket,1,1,&words(&[2]))?;send(&mut socket,1,0,&words(&[3]))?;
    let mut globals=std::collections::BTreeMap::new();
    loop {
        let (object,opcode,data)=event(&mut socket)?;
        if object==3 {break;}
        if object==2 && opcode==0 {
            let length=number(&data,4) as usize;
            let interface=std::str::from_utf8(data.get(8..8+length.saturating_sub(1)).ok_or("invalid global")?).map_err(|e|e.to_string())?;
            globals.insert(interface.to_owned(),number(&data,0));
        }
    }
    for (interface,id) in [("wl_compositor",4),("wl_shm",5),("xdg_wm_base",6)] {
        bind(&mut socket,*globals.get(interface).ok_or("required Wayland global is missing")?,interface,id)?;
    }
    send(&mut socket,4,0,&words(&[7]))?;
    send(&mut socket,6,2,&words(&[8,7]))?;
    send(&mut socket,8,1,&words(&[9]))?;
    send(&mut socket,9,2,&string("Rust fullscreen fixture"))?;
    send(&mut socket,9,3,&string("axia.switcher-probe"))?;
    send(&mut socket,9,11,&words(&[0]))?;send(&mut socket,7,6,&[])?;
    let mut dimensions=(640u32,480u32);let mut mapped=false;
    loop {
        let (object,opcode,data)=event(&mut socket)?;
        if object==6 && opcode==0 {send(&mut socket,6,3,&data[..4])?;}
        if object==9 && opcode==1 {return Ok(());}
        if object==9 && opcode==0 && !mapped {
            let (w,h)=(number(&data,0),number(&data,4));
            if w>0 && h>0 && w<=8192 && h<=8192 {dimensions=(w,h);}
        }
        if object==8 && opcode==0 {
            send(&mut socket,8,4,&data[..4])?;
            if !mapped {
                let (w,h)=dimensions;let size=(w*h*4) as usize;
                let fd=unsafe{libc::memfd_create(c"switcher-fullscreen".as_ptr(),libc::MFD_CLOEXEC)};
                if fd<0{return Err(std::io::Error::last_os_error().to_string());}
                let mut file=unsafe{fs::File::from_raw_fd(fd)};
                file.set_len(size as u64).map_err(|e|e.to_string())?;
                file.write_all(&vec![0x40;size]).map_err(|e|e.to_string())?;
                pool(&mut socket,file.as_raw_fd(),size)?;
                send(&mut socket,10,0,&words(&[11,0,w,h,w*4,1]))?;
                send(&mut socket,7,1,&words(&[11,0,0]))?;
                send(&mut socket,7,2,&words(&[0,0,w,h]))?;
                mapped=true;
            }
            send(&mut socket,7,6,&[])?;
        }
        if Instant::now()>=until{return Err("fullscreen fixture did not close".into());}
    }
}
