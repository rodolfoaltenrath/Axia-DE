// Raw disk facts and closed platform operations for the native utility.
// Rust owns selection, SMART interpretation, confirmation and progress policy.
export function diskServices({disks, admin, fs, systemInfo, smart, smartSummaries, events}) {
  const validName = value => typeof value === 'string' && /^[a-z][a-z0-9]{0,30}$/.test(value);
  const smartName = value => typeof value === 'string' && /^(sd[a-z]{1,3}|hd[a-z]{1,3}|nvme\d{1,3}n\d{1,3})$/.test(value);
  const collect = async (action, arg, src) => {
    const lines=[]; let length=0,overflow=false;
    const start=Date.now();
    const status=await admin.run(action,arg,line=>{
      if(overflow) return;
      const value=String(line);length+=value.length;
      if(length>4*1024*1024||lines.length>=8192) {overflow=true;return;}
      lines.push(value);
    },{quiet:true});
    if(overflow) throw Error('disk.smartFailed');
    return {src,lines,status:status??'',ms:Date.now()-start};
  };
  const read = (action,src='smartctl',own=false) => ({run([name]) {
    if(!(own?validName(name):smartName(name))) throw Error('disk.badDevice');
    return collect(action,name,src);
  }});
  const actions={mount:'disk-mount',unmount:'disk-unmount',eject:'disk-eject',format:'disk-format',label:'disk-label',erase:'disk-erase',image:'disk-write-image'};
  return {
    'disks.snapshot': {run:()=>({available:disks.available(),list:disks.list(),tools:disks.tools(),admin:!!systemInfo()?.admin&&admin.available(),home:fs.home()})},
    'disks.run': {run([action,arg,generation],owner) {
      if(!Object.hasOwn(actions,action)||typeof arg!=='string'||arg.length>8192||!validName(arg.split('\n')[0])) throw Error('app.invalidRequest');
      return admin.run(actions[action],arg,line=>owner.emit({kind:'progress',generation,line:String(line).slice(0,8192)}),{quiet:true});
    }},
    'disks.events': {run:(_,owner)=>events(owner.id)},
    'disks.cancel': {run([name]) {if(!validName(name)) throw Error('disk.badDevice');return admin.run('disk-cancel',name,null,{quiet:true});}},
    'disks.images': {run() {
      const found=[];
      for(const ext of ['.iso','.img']) {try {found.push(...fs.search(fs.home(),ext,{limit:40,depth:4}));}catch(_) {}}
      return found;
    }},
    'disks.stat': {run([path]) {if(typeof path!=='string'||path.length>4096) return null;try {return path?fs.stat(path):null;}catch(_) {return null;}}},
    'disks.smartInfo':read('smart-info'), 'disks.smartHealth':read('smart-health'), 'disks.smartOwn':read('disk-smart','own',true),
    'disks.smartAbort':read('smart-abort'),
    'disks.smartTest': {run([name,kind]) {if(!smartName(name)||!['short','long'].includes(kind)) throw Error('app.invalidRequest');return collect('smart-test',`${name}\n${kind}`,'smartctl');}},
    'disks.installSmart': {async run() {await smart.install();return {tools:disks.tools()};}},
    'disks.summary': {run([name,summary]) {if(!validName(name)) throw Error('disk.badDevice');smartSummaries.set(all=>({...all,[name]:summary}));return null;}},
  };
}
