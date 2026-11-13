//! Returned preview/thumbnail handles belong to the native Files window, not
//! its shell adapter. Rejected late results and window teardown release them.
use serde_json::Value;
use std::collections::BTreeSet;

#[derive(Default)]
pub(super) struct Resources { media:BTreeSet<i32>, images:BTreeSet<i32> }
fn id(v:&Value)->Option<i32> { v.as_i64().and_then(|id|i32::try_from(id).ok()).filter(|id|*id>0) }
fn returned(result:&Value)->(Option<i32>,Option<i32>) {
    match result["kind"].as_str() {
        Some("mediaPreview")=>(id(&result["result"]),None),
        Some("documentPreview"|"pdfThumbnail")=>(None,id(&result["result"]["id"])),
        _=>(None,None),
    }
}
impl Resources {
    pub fn observe(&mut self,result:&Value) {
        let (media,image)=returned(result);
        if let Some(id)=media {self.media.insert(id);}
        if let Some(id)=image {self.images.insert(id);}
    }
    pub fn close(&mut self,effect:&Value) {
        if let Some(id)=id(&effect["media"]) {self.media.remove(&id);release_media(id);}
        if let Some(id)=id(&effect["image"]["id"]) {self.images.remove(&id);release_image(id);}
    }
    pub fn clear(&mut self) {
        for id in std::mem::take(&mut self.media) {release_media(id);}
        for id in std::mem::take(&mut self.images) {release_image(id);}
    }
}
impl Drop for Resources {fn drop(&mut self){self.clear();}}
pub(crate) fn discard_result(result:&Value) {
    let (media,image)=returned(result);
    if let Some(id)=media {release_media(id);}
    if let Some(id)=image {release_image(id);}
}
#[cfg(not(test))]
fn release_media(id:i32) {
    extern "C" {
        fn media_src_get(id:i32)->*mut std::ffi::c_void;
        fn media_src_close(source:*mut std::ffi::c_void);
    }
    unsafe { let source=media_src_get(id); if !source.is_null() {media_src_close(source);} }
}
#[cfg(not(test))]
fn release_image(id:i32) { extern "C" {fn doc_image_release(id:i32);} unsafe {doc_image_release(id);} }
#[cfg(test)]
thread_local! {static RELEASED:std::cell::RefCell<Vec<(bool,i32)>>=Default::default();}
#[cfg(test)]
fn release_media(id:i32) {RELEASED.with(|r|r.borrow_mut().push((true,id)));}
#[cfg(test)]
fn release_image(id:i32) {RELEASED.with(|r|r.borrow_mut().push((false,id)));}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn close_window_and_late_results_release_each_owned_resource_once() {
        RELEASED.with(|r|r.borrow_mut().clear());
        {
            let mut resources=Resources::default();
            resources.observe(&json!({"kind":"mediaPreview","result":12}));
            resources.observe(&json!({"kind":"documentPreview","result":{"id":34}}));
            resources.observe(&json!({"kind":"pdfThumbnail","result":{"id":56}}));
            resources.close(&json!({"media":12}));
            resources.close(&json!({"image":{"id":34}}));
        }
        let late=std::ffi::CString::new(json!({"kind":"mediaPreview","result":78}).to_string()).unwrap();
        unsafe {crate::native_view::axia_view_result(u32::MAX,late.as_ptr());}
        discard_result(&json!({"kind":"documentPreview","result":{"id":90}}));
        discard_result(&json!({"kind":"diskMount","result":123}));
        RELEASED.with(|r|assert_eq!(&*r.borrow(),&[(true,12),(false,34),(false,56),(true,78),(false,90)]));
    }
}
