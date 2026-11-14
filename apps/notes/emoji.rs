//! Original bilingual emoji catalog, compiled into the application.
use std::sync::OnceLock;
pub const CATEGORIES: [&str; 8] = [
    "caras",
    "gente",
    "simbolos",
    "objetos",
    "comida",
    "naturaleza",
    "lugares",
    "banderas",
];
const RAW: &str = r#"
😀 grinning,smile sonrisa,feliz caras
😃 smiley,happy alegre caras
😄 smile,laugh risa caras
😁 grin mueca,sonrisa caras
😆 laughing,lol carcajada caras
😅 sweat_smile,phew alivio caras
😂 joy,tears_of_joy llorar_de_risa caras
🤣 rofl revolcarse caras
🙂 slight_smile sonrisita caras
🙃 upside_down al_reves caras
😉 wink guiño caras
😊 blush sonrojado caras
😇 innocent,angel angel,inocente caras
🥰 smiling_hearts enamorado caras
😍 heart_eyes ojos_corazon caras
🤩 star_struck deslumbrado caras
😘 kiss beso caras
😋 yum delicioso caras
😛 tongue lengua caras
😜 wink_tongue guiño_lengua caras
🤪 zany loco caras
🤔 thinking,hmm pensando caras
🤨 raised_eyebrow ceja caras
😐 neutral neutral caras
😑 expressionless sin_expresion caras
😶 no_mouth sin_boca caras
🙄 eye_roll revolear_ojos caras
😏 smirk picaro caras
😬 grimacing incomodo caras
😌 relieved aliviado caras
😔 pensive pensativo caras
😪 sleepy sueño caras
😴 sleeping dormido caras
😷 mask barbijo,mascara caras
🤒 sick enfermo caras
🤯 exploding_head cabeza_explota caras
🥳 party fiesta caras
😎 sunglasses,cool lentes,genial caras
🤓 nerd nerd caras
😕 confused confundido caras
😟 worried preocupado caras
😮 open_mouth sorpresa caras
😲 astonished asombrado caras
😳 flushed avergonzado caras
🥺 pleading suplica caras
😢 cry llorar caras
😭 sob llanto caras
😱 scream grito,miedo caras
😤 triumph bufido caras
😡 angry,rage enojado,furia caras
🤬 cursing insultos caras
💀 skull calavera caras
💩 poop caca caras
🤡 clown payaso caras
👻 ghost fantasma caras
👽 alien alien caras
🤖 robot robot caras
👍 thumbs_up,ok,yes pulgar_arriba,si,bien gente
👎 thumbs_down,no pulgar_abajo gente
👌 ok_hand perfecto gente
✌️ victory paz,victoria gente
🤞 fingers_crossed dedos_cruzados,suerte gente
🤘 rock rock gente
👋 wave,hello,hi hola,saludo gente
👏 clap aplauso gente
🙌 raised_hands celebrar gente
🙏 pray,please,thanks gracias,por_favor gente
💪 muscle,strong fuerza gente
👀 eyes ojos,mirar gente
🧠 brain cerebro gente
👶 baby bebe gente
🧑 person persona gente
👨‍💻 technologist,developer programador gente
🧑‍🏫 teacher profesor gente
❤️ heart,love corazon,amor simbolos
🧡 orange_heart corazon_naranja simbolos
💛 yellow_heart corazon_amarillo simbolos
💚 green_heart corazon_verde simbolos
💙 blue_heart corazon_azul simbolos
💜 purple_heart corazon_violeta simbolos
🖤 black_heart corazon_negro simbolos
💔 broken_heart corazon_roto simbolos
💯 hundred,100 cien simbolos
✅ check,done,yes listo,hecho,tilde simbolos
✔️ check_mark tilde simbolos
☑️ ballot_box casilla simbolos
❌ x,cross,no cruz,no simbolos
⚠️ warning atencion,advertencia simbolos
⛔ no_entry prohibido simbolos
🚫 forbidden prohibido simbolos
❗ exclamation exclamacion simbolos
❓ question pregunta simbolos
💡 idea,bulb idea,bombilla simbolos
⭐ star estrella simbolos
🌟 glowing_star estrella_brillante simbolos
✨ sparkles brillos simbolos
🔥 fire,hot fuego simbolos
⚡ zap,lightning rayo simbolos
💥 boom explosion simbolos
🎯 target,goal objetivo,meta simbolos
🔴 red_circle rojo simbolos
🟠 orange_circle naranja simbolos
🟡 yellow_circle amarillo simbolos
🟢 green_circle verde simbolos
🔵 blue_circle azul simbolos
🟣 purple_circle violeta simbolos
⚫ black_circle negro simbolos
⚪ white_circle blanco simbolos
➡️ arrow_right flecha_derecha simbolos
⬅️ arrow_left flecha_izquierda simbolos
⬆️ arrow_up flecha_arriba simbolos
⬇️ arrow_down flecha_abajo simbolos
🔁 repeat repetir simbolos
♻️ recycle reciclar simbolos
➕ plus mas simbolos
➖ minus menos simbolos
📌 pin,pushpin chinche,fijar objetos
📍 location ubicacion objetos
📎 paperclip,attachment clip,adjunto objetos
🔗 link enlace objetos
📝 memo,note nota,apunte objetos
📒 notebook cuaderno objetos
📚 books libros objetos
📖 book libro objetos
✏️ pencil lapiz objetos
🖊️ pen lapicera objetos
📅 calendar calendario objetos
🗓️ spiral_calendar agenda objetos
⏰ alarm,clock alarma,reloj objetos
⏳ hourglass reloj_de_arena objetos
⌛ hourglass_done tiempo objetos
📞 phone telefono objetos
📱 mobile celular objetos
💻 laptop,computer computadora,notebook objetos
🖥️ desktop pantalla objetos
⌨️ keyboard teclado objetos
🖱️ mouse mouse objetos
💾 floppy,save disquete,guardar objetos
📁 folder carpeta objetos
📂 open_folder carpeta_abierta objetos
🗂️ dividers fichero objetos
📄 page,document documento,hoja objetos
📊 chart,bar_chart grafico objetos
📈 trending_up subida objetos
📉 trending_down bajada objetos
🧾 receipt recibo objetos
💰 money,bag dinero objetos
💳 card tarjeta objetos
🔒 lock candado,cerrado objetos
🔓 unlock abierto objetos
🔑 key llave,clave objetos
🛠️ tools herramientas objetos
🔧 wrench llave_inglesa objetos
⚙️ gear,settings engranaje,ajustes objetos
🧪 test,lab prueba,laboratorio objetos
🐛 bug bicho,error objetos
🚀 rocket,launch cohete,lanzar objetos
📦 package paquete objetos
🎁 gift regalo objetos
🎉 tada,party fiesta,celebracion objetos
🎂 cake torta,cumpleaños objetos
🏆 trophy trofeo objetos
🎵 music musica objetos
🎮 game videojuego objetos
📷 camera camara objetos
🔔 bell campana,aviso objetos
✉️ email,mail correo objetos
📢 loudspeaker anuncio objetos
🧹 broom escoba,limpiar objetos
🛒 cart carrito,compras objetos
☕ coffee cafe comida
🍵 tea te comida
🧉 mate mate comida
🍕 pizza pizza comida
🍔 burger hamburguesa comida
🍎 apple manzana comida
🍌 banana banana comida
🍰 cake torta comida
🍷 wine vino comida
🍺 beer cerveza comida
🌞 sun sol naturaleza
🌙 moon luna naturaleza
⛅ cloud nube naturaleza
🌧️ rain lluvia naturaleza
❄️ snow nieve naturaleza
🌈 rainbow arcoiris naturaleza
🌱 seedling brote naturaleza
🌳 tree arbol naturaleza
🌸 blossom flor naturaleza
🍀 clover trebol,suerte naturaleza
🐶 dog perro naturaleza
🐱 cat gato naturaleza
🐧 penguin,linux pinguino naturaleza
🦊 fox zorro naturaleza
🐢 turtle tortuga naturaleza
🏠 house casa lugares
🏢 office oficina lugares
🏫 school escuela lugares
🏥 hospital hospital lugares
✈️ airplane avion lugares
🚗 car auto lugares
🚲 bike bici lugares
🗺️ map mapa lugares
🌍 globe,earth mundo lugares
🇦🇷 argentina argentina banderas
🇧🇷 brazil brasil banderas
🇪🇸 spain españa banderas
🇺🇸 usa estados_unidos banderas
🇲🇽 mexico mexico banderas
🇨🇱 chile chile banderas
🇺🇾 uruguay uruguay banderas
🏳️ white_flag bandera_blanca banderas
🏁 checkered_flag llegada banderas
"#;
#[derive(Clone, Debug)]
pub struct Emoji {
    pub ch: String,
    pub names: Vec<String>,
    pub name: String,
    pub cat: String,
}
pub fn all() -> &'static Vec<Emoji> {
    static DATA: OnceLock<Vec<Emoji>> = OnceLock::new();
    DATA.get_or_init(|| {
        RAW.lines()
            .filter_map(|l| {
                let p: Vec<_> = l.split_whitespace().collect();
                if p.len() != 4 {
                    return None;
                }
                Some(Emoji {
                    ch: p[0].into(),
                    names: p[1]
                        .split(',')
                        .chain(p[2].split(','))
                        .map(str::to_owned)
                        .collect(),
                    name: p[1].split(',').next().unwrap().into(),
                    cat: p[3].into(),
                })
            })
            .collect()
    })
}
pub fn get(name: &str) -> Option<&str> {
    all()
        .iter()
        .find(|e| e.names.iter().any(|n| n == name))
        .map(|e| e.ch.as_str())
}
pub fn search(q: &str) -> Vec<Emoji> {
    let q = q.to_lowercase();
    if q.is_empty() {
        return all().clone();
    }
    let mut start = vec![];
    let mut contains = vec![];
    for e in all() {
        if let Some(name) = e.names.iter().find(|n| n.starts_with(&q)) {
            let mut v = e.clone();
            v.name = name.clone();
            start.push(v);
        } else if let Some(name) = e.names.iter().find(|n| n.contains(&q)) {
            let mut v = e.clone();
            v.name = name.clone();
            contains.push(v);
        }
    }
    start.extend(contains);
    start
}
pub fn expand(source: &str) -> String {
    let re = regex::Regex::new(r":([a-zA-Z0-9_+-]+):").unwrap();
    re.replace_all(source, |c: &regex::Captures| {
        get(&c[1]).unwrap_or(&c[0]).to_owned()
    })
    .into_owned()
}
