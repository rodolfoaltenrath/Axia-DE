pub const ID: &str = "pdf";
pub fn manifest() -> serde_json::Value {
    serde_json::json!({"id":ID,"version":"0.8.5","name":"PDF Viewer",
    "description":"Preview-style PDF viewer: thumbnails, outline, search, annotations, forms and printing",
    "size":[980,680],"minSize":[560,380],"dock":false,"category":"Office",
    "permissions":["fs:home:read","fs:home:write","print"],
    "nativeView":"rust","appearanceSource":"native","keyboard":true,
    "asyncServices":["pdf.recent","pdf.open","pdf.sizes","pdf.render","pdf.release","pdf.close","pdf.outline","pdf.pageInfo",
        "pdf.select","pdf.find","pdf.events","pdf.annot","pdf.setField","pdf.undo","pdf.save","pdf.print",
        "clipboard.write","windows.open","windows.menus","windows.state"],
    "windowState":true,
    "calls":{"nativeAsync":["start"]},"reads":{"nativeAsync":["take"]}})
}
