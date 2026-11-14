'use strict';
const assert=require('node:assert/strict'),fs=require('node:fs'),vm=require('node:vm');
const source=fs.readFileSync('crates/seiya-server/src/desktop_setup_feedback.js','utf8');
function fixture(){
 const nodes=new Map(),observers=[];let requests=0,inputs=0;
 class Element {
  constructor(id=''){this.id=id;this.textContent='';this.value='';this.hidden=true;this.disabled=false;this.events={};this.scrolls=0;this.focused=false;}
  setAttribute(){}
  insertAdjacentElement(where,node){assert.equal(where,'afterend');nodes.set(node.id,node);}
  addEventListener(type,fn){(this.events[type]??=[]).push(fn);}
  dispatchEvent(event){for(const fn of this.events[event.type]||[])fn(event);return true;}
  scrollIntoView(){this.scrolls++;}
  focus(){this.focused=true;}
 }
 for(const id of ['setupForm','setupReview','setupMessage','setupApproval','setupBrain','setupImage','setupImageBinary','setupImageAssets','setupLlm','setupStore','setupWorkers'])nodes.set(id,new Element(id));
 nodes.get('setupLlm').value='/preserved/ollama';nodes.get('setupStore').value='/preserved/models';nodes.get('setupWorkers').value='preserved-worker';
 nodes.get('setupImage').value='qwen-image-2.1-turbo-q4';nodes.get('setupImageBinary').value='/old/image';nodes.get('setupImageAssets').value='/old/assets';
 nodes.get('setupForm').addEventListener('input',()=>{inputs++;nodes.get('setupApproval').hidden=true;});
 const context={document:{getElementById:id=>nodes.get(id)||null,createElement:()=>new Element()},
  MutationObserver:class{constructor(fn){observers.push(fn);}observe(node){assert.equal(node.id,'setupMessage');}},
  Event:class{constructor(type,options){this.type=type;this.bubbles=options.bubbles;}},fetch(){requests++;throw Error('No network permitted');}};
 vm.runInNewContext(source,context);
 return {nodes,notify:()=>observers.forEach(fn=>fn()),requests:()=>requests,inputs:()=>inputs};
}
const f=fixture(),get=id=>f.nodes.get(id);
assert.equal(f.requests(),0);assert.equal(get('setupReviewFeedback').scrolls,0);
get('setupForm').dispatchEvent({type:'submit'});
assert.match(get('setupReviewFeedback').textContent,/Revisando/);
get('setupMessage').textContent='images.binary:absolute_path_required No se reintentó la acción.';f.notify();
assert.match(get('setupReviewFeedback').textContent,/sd-cli/);assert.match(get('setupReviewFeedback').textContent,/images.binary:absolute_path_required/);assert.equal(get('setupReviewFeedback').scrolls,1);
f.notify();assert.equal(get('setupReviewFeedback').scrolls,1,'Polling status must not keep moving the page');
get('setupForm').dispatchEvent({type:'submit'});get('setupApproval').hidden=false;get('setupMessage').textContent='Revisión de rutas completada.';f.notify();assert.equal(get('setupApproval').scrolls,1);
get('setupForm').dispatchEvent({type:'invalid',target:get('setupBrain')});assert.match(get('setupReviewFeedback').textContent,/Cerebro/);assert.ok(get('setupBrain').focused);
get('setupWithoutImages').dispatchEvent({type:'click'});f.notify();
for(const id of ['setupImage','setupImageBinary','setupImageAssets'])assert.equal(get(id).value,'');
assert.equal(get('setupLlm').value,'/preserved/ollama');assert.equal(get('setupStore').value,'/preserved/models');assert.equal(get('setupWorkers').value,'preserved-worker');assert.equal(f.inputs(),1);assert.ok(get('setupApproval').hidden);assert.equal(f.requests(),0);
get('setupMessage').textContent='<img src=x onerror=evil()> model_store:existing_path_required';f.notify();assert.match(get('setupReviewFeedback').textContent,/Instalar Seiya no instala pesos/);assert.match(get('setupReviewFeedback').textContent,/<img/);assert.equal(f.requests(),0);
console.log('Setup feedback: visible errors/approval, required focus, no scroll loop, explicit text-only draft, invalidation and zero network/model side effects OK (DOM mocks)');
