'use strict';
const assert=require('node:assert/strict'),vm=require('node:vm'),fs=require('node:fs');
const source=fs.readFileSync('crates/seiya-server/src/chat_images_ui.js','utf8');
function fixture(){
 const chat={id:'conversation'},calls=[],events={},status={textContent:''};let fail=false,saves=true,counter=0;
 const context={activeChat:()=>chat,$:id=>id==='deleteChat'?{addEventListener:(n,fn)=>events[n]=fn}:status,
  crypto:{randomUUID:()=>`11111111-1111-4111-8111-${String(++counter).padStart(12,'0')}`},
  saveChats:()=>saves,seiyaUuid:s=>typeof s==='string'&&/^[a-f0-9-]{36}$/.test(s),
  seiyaImageSession:async()=> 'private-image-session',seiyaImageLabels:{queued:'queued'},seiyaRefreshImages:async()=>{},
  seiyaChatFetch:async(headers,payload)=>{calls.push(payload);if(fail)throw Error('uncertain');return {json:async()=>({image_job:{job_id:'22222222-2222-4222-8222-222222222222',request_id:payload.image_request_id,phase:'queued'}})}}};
 vm.createContext(context);vm.runInContext(source,context);
 return {context,chat,calls,events,status,fail:v=>fail=v,saves:v=>saves=v};
}
(async()=>{
 const f=fixture();assert.equal(f.calls.length,0,'Loading extension must not send anything');
 const payload={model:'pegassus',messages:[{role:'user',content:'Create a blue horse'}]};
 f.fail(true);await assert.rejects(f.context.seiyaChatFetch({},payload),/uncertain/);
 const id=f.chat.chatImagePending.id;assert.equal(f.calls[0].image_session,'private-image-session');
 let stopped=false;f.events.click({preventDefault(){},stopImmediatePropagation(){stopped=true}});assert.ok(stopped);
 await assert.rejects(f.context.seiyaChatFetch({},{...payload,messages:[{role:'user',content:'Different request'}]}),/sin confirmar/);assert.equal(f.calls.length,1);
 f.fail(false);await f.context.seiyaChatFetch({},payload);assert.equal(f.calls[1].image_request_id,id);assert.equal(f.chat.chatImagePending,undefined);
 const quota=fixture();quota.saves(false);await assert.rejects(quota.context.seiyaChatFetch({},payload),/referencia/);assert.equal(quota.calls.length,0);
 const reload=fixture();reload.chat.chatImagePending={id,text:'Create a blue horse'};assert.equal(reload.calls.length,0);await reload.context.seiyaChatFetch({},payload);assert.equal(reload.calls[0].image_request_id,id);
 console.log('Chat images: no load/reload replay, persist-before-send, private reference, same-ID recovery, uncertain deletion block and quota rejection OK (mocks)');
})().catch(e=>{console.error(e);process.exitCode=1});
