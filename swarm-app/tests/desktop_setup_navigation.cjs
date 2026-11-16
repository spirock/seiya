// Initial shell navigation; synthetic DOM only, no APIs, storage or models.
const fs=require('node:fs'),vm=require('node:vm'),assert=require('node:assert/strict');
class Element{
 constructor(){this.children=[];this.hidden=false;this.attrs={};this.style={};this.listeners={};this.value='';}
 append(...nodes){for(const node of nodes){if(node.parentNode)node.parentNode.children=node.parentNode.children.filter(c=>c!==node);this.children.push(node);node.parentNode=this;}}
 setAttribute(k,v){this.attrs[k]=v;}focus(){this.focused=true;}addEventListener(k,fn){this.listeners[k]=fn;}
}
const els={},get=id=>els[id]??=new Element(),aside=new Element();
get('scroll').append(get('welcome'),get('history'));get('composer').append(get('form'));get('prompt').value='preserved';get('scroll').scrollTop=73;
const ctx={document:{getElementById:get,createElement:()=>new Element(),querySelector:s=>{assert.equal(s,'aside');return aside;}},Map};
vm.createContext(ctx);vm.runInContext(fs.readFileSync('crates/seiya-server/src/desktop_setup_navigation.js','utf8'),ctx);
const run=s=>vm.runInContext(s,ctx),[root,menu]=aside.children,[back,surface,setup,general]=get('scroll').children;
assert.equal(menu.hidden,true);assert.equal(setup.hidden,true);assert.equal(general.hidden,true);assert.equal(surface.hidden,false);
root.onclick();assert.equal(menu.hidden,false);menu.children[0].onclick();
assert.equal(setup.parentNode,get('scroll'));assert.equal(setup.hidden,false);assert.equal(surface.hidden,true);assert.equal(get('composer').hidden,true);assert.equal(general.hidden,true);
get('setupBrain').value='edited';menu.children[1].onclick();assert.equal(general.hidden,false);assert.equal(setup.hidden,true);assert.equal(surface.hidden,true);assert.equal(get('composer').hidden,true);
back.onclick();assert.equal(surface.hidden,false);assert.equal(get('composer').hidden,false);assert.equal(get('scroll').scrollTop,73);assert.equal(get('prompt').value,'preserved');assert.equal(get('setupBrain').value,'edited');
menu.children[0].onclick();setup.listeners.keydown({key:'Escape',preventDefault(){}});assert.equal(setup.hidden,true);
menu.children[0].onclick();run('seiyaSetupSaved()');assert.equal(setup.hidden,true);assert.match(get('status').textContent,/guardada sin iniciar/);
menu.children[0].onclick();get('clear').listeners.click();assert.equal(surface.hidden,false);
assert.ok(fs.readFileSync('crates/seiya-server/src/desktop_setup.rs','utf8').includes('max-width:780px;margin:0 auto;'));
console.log('Initial settings: no popup, exclusive chat/settings surface, submenu, Escape/back/save and preserved draft (synthetic DOM).');
