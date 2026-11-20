// Explicit click and DOM-only mocks; no network.
const fs=require('node:fs'),vm=require('node:vm'),assert=require('node:assert/strict');
class Element{
 constructor(){this.children=[];this.handlers={};this.textContent='';this.disabled=false;}
 append(...nodes){this.children.push(...nodes);}
 setAttribute(){}
 querySelector(selector){return this.children.find(c=>selector==='#'+c.id)||null;}
 addEventListener(type,handler){this.handlers[type]=handler;}
}
(async()=>{
 const root=new Element(),calls=[];
 let response={ok:true,json:async()=>({available:true,version:'1.2.3',release_url:'https://github.com/spirock/seiya/releases/tag/v1.2.3'})};
 const ctx={document:{getElementById:id=>id==='setup'?root:null,createElement:()=>new Element()},location:{href:'http://127.0.0.1:8015/app/example/'},URL,
   fetch:async(url,options)=>{calls.push({url:String(url),options});return response;}};
 vm.createContext(ctx);
 vm.runInContext(fs.readFileSync('crates/seiya-server/src/update_ui.js','utf8'),ctx);
 vm.runInContext('seiyaMountUpdate(location.href); seiyaMountUpdate(location.href)',ctx);
 assert.equal(calls.length,0);
 assert.equal(root.children.length,1);
 const [title,note,button,status]=root.children[0].children;
 assert.match(note.textContent,/GitHub.*IP/);
 await button.handlers.click();
 assert.equal(calls.length,1);
 assert.equal(calls[0].url,'http://127.0.0.1:8015/app/example/api/update-check');
 assert.equal(JSON.parse(calls[0].options.body).consent,true);
 assert.equal(status.children[0].href,'https://github.com/spirock/seiya/releases/tag/v1.2.3');
 response={ok:true,json:async()=>({available:true,version:'1.2.3',release_url:'https://evil.invalid/'})};
 await button.handlers.click();
 assert.match(status.textContent,/no válida/);
 console.log('Updates UI: no network on load, explicit consent, safe link and untrusted URL rejection (synthetic DOM).');
})().catch(e=>{console.error(e);process.exitCode=1;});
