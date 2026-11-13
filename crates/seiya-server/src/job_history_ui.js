// PreparedWeb-only recovery. Browser chat UUID is a reference, never a grant.
let seiyaJobHistoryEnabled=false,seiyaJobHistoryDurable=false,seiyaHistoryDeleting=false;
let seiyaHistoryAuthBlocked=false;
// Aborting HTTP never proves runtime cancellation. This only bounds UI waiting.
async function seiyaControlFetch(work,timeoutMs=15000){
 const controller=new AbortController();let timer;
 try{return await Promise.race([work(controller.signal),new Promise((_,reject)=>{timer=setTimeout(()=>{controller.abort();reject(Error('Se agotó el plazo de conexión. Estado incierto; consulta antes de reenviar.'));},timeoutMs);})]);}
 finally{clearTimeout(timer);}
}
function seiyaHttpError(response,data){
 const e=Error(runtimeError(data,response.statusText||'Error de conexión'));e.status=response.status;
 e.confirmed_rejection=[400,401,403,404,409,413,415,429,503].includes(response.status);
 if(response.status===401||response.status===403){seiyaHistoryAuthBlocked=true;e.message='Acceso caducado o rechazado. Autentica de nuevo y pulsa Revisar conexión; no se reenviará nada.';}
 return e;
}
function seiyaValidJobList(data,limit,labels){
 if(!Array.isArray(data.jobs)||data.jobs.length>limit||data.jobs.some(j=>!j||typeof j.job_id!=='string'||!seiyaUuid(j.job_id)||!Object.hasOwn(labels,j.phase)||typeof j.artifact_available!=='boolean'||j.artifact_available!==(j.phase==='completed')))throw Error('Estado de trabajos inválido. No se confirma ningún resultado.');
 return data;
}
function seiyaUuid(s){return typeof s==='string'&&/^[a-f0-9]{8}-[a-f0-9]{4}-[a-f0-9]{4}-[a-f0-9]{4}-[a-f0-9]{12}$/.test(s);}
jobControl=async function(body){return seiyaControlFetch(async signal=>{
 const headers={'Content-Type':'application/json'};if($('key').value)headers.Authorization='Bearer '+$('key').value;
 const r=await fetch('/api/jobs',{method:'POST',headers,signal,body:JSON.stringify(body)});let data;try{data=await r.json();}catch{if(r.ok)throw Error('Respuesta de trabajos inválida; consulta estado antes de reenviar.');}
 if(!r.ok)throw seiyaHttpError(r,data);return data;
});};
async function seiyaChatFetch(headers,payload){return seiyaControlFetch(async signal=>{
 const r=await fetch('/api/chat',{method:'POST',headers,signal,body:JSON.stringify(payload)});let data;try{data=await r.json();}catch{throw Error('Respuesta de chat inválida; estado incierto, no reenvíes sin comprobar.');}
 if(!r.ok)throw seiyaHttpError(r,data);return{ok:true,json:async()=>data};
},150000);}
async function seiyaHealthFetch(headers){return seiyaControlFetch(async signal=>{
 const r=await fetch('/health',{headers,signal});let data;try{data=await r.json();}catch{throw Error('Estado del host inválido.');}if(!r.ok)throw seiyaHttpError(r,data);return{ok:true,json:async()=>data};
});}
const seiyaJobOpenRequests=new Map();
function seiyaJobHistoryCapabilities(cap){seiyaJobHistoryEnabled=cap?.history_enabled===true||cap?.enabled===true;seiyaJobHistoryDurable=cap?.volatile===false;if(seiyaJobHistoryEnabled)refreshJobs();}
jobSession=async function(id){
 if(jobSessions.has(id))return jobSessions.get(id);
 if(!seiyaJobOpenRequests.has(id))seiyaJobOpenRequests.set(id,(async()=>{const r=await jobControl({operation:'open',conversation_id:id});if(typeof r.session!=='string'||!/^[a-f0-9]{64}$/.test(r.session))throw Error('Sesión de trabajos inválida.');jobSessions.set(id,r.session);return r.session;})().finally(()=>seiyaJobOpenRequests.delete(id)));
 return seiyaJobOpenRequests.get(id);
};
refreshJobs=async function(){
 if(!seiyaJobHistoryEnabled||jobsPolling||seiyaHistoryDeleting||seiyaHistoryAuthBlocked)return;
 const id=activeId;if(!id)return;jobsPolling=true;
 try{const session=await jobSession(id);const data=await jobControl({operation:'list',session});
  seiyaValidJobList(data,64,jobLabels);
  renderJobs(data,id,session);
  if(id===activeId){const note=document.createElement('p');note.textContent=seiyaJobHistoryDurable?'Historial local persistente. Reiniciar no reenvía trabajos.':'Historial del host temporal; recargar recupera enlaces mientras viva el host.';jobsPanel.append(note);}
 }catch(e){if(id===activeId){jobsPanel.hidden=false;jobsPanel.textContent=e.message+' No se reenviará ningún trabajo.';jobSessions.delete(id);}}
 finally{jobsPolling=false;}
};
setInterval(refreshJobs,2000);
const seiyaLegacyRenderJobs=renderJobs;
renderJobs=function(data,chatId,session){
 seiyaValidJobList(data,64,jobLabels);seiyaLegacyRenderJobs({...data,jobs:data.jobs.map(j=>({...j,artifact_available:false}))},chatId,session);
 if(chatId!==activeId)return;
 for(const job of data.jobs.filter(j=>j.phase==='completed')){
  const row=[...jobsPanel.children].find(c=>c.attrs?.['data-job-id']===job.job_id||c.getAttribute?.('data-job-id')===job.job_id);
  if(!row)continue;const button=document.createElement('button');button.type='button';button.textContent='Descargar borrador';
  button.onclick=async()=>{if(button.disabled)return;button.disabled=true;let url;
   try{const artifact=await jobControl({operation:'artifact',session,job_id:job.job_id});if(typeof artifact.text!=='string'||new TextEncoder().encode(artifact.text).length>65536)throw Error('Borrador inválido.');if(activeId!==chatId||!chats.some(c=>c.id===chatId))throw Error('La conversación cambió; descarga detenida.');url=URL.createObjectURL(new Blob([artifact.text],{type:'text/plain;charset=utf-8'}));const a=document.createElement('a');a.href=url;a.download='borrador.txt';row.append(a);a.click();a.remove();}
   catch(e){$('status').textContent=e.message;}finally{if(url)URL.revokeObjectURL(url);button.disabled=false;}
  };row.append(button);
 }
};
const seiyaLegacyOpenChat=openChat;
openChat=function(id){
 if(seiyaHistoryDeleting)return;
 if(id!==activeId){jobsPanel.hidden=true;if(typeof seiyaImagePanel!=='undefined')seiyaImagePanel.hidden=true;}
 seiyaLegacyOpenChat(id);
 if(typeof seiyaSetImageMode==='function'&&!busy)seiyaSetImageMode(Boolean(activeChat()?.imagePending));
 refreshJobs();if(typeof seiyaRefreshImages==='function')seiyaRefreshImages();
};
const seiyaLegacyCheckConnection=$('checkRuntime').onclick;
async function seiyaReviewConnection(){seiyaHistoryAuthBlocked=false;jobSessions.clear();if(typeof seiyaImageSessions!=='undefined')seiyaImageSessions.clear();await seiyaLegacyCheckConnection();await refreshJobs();if(typeof seiyaRefreshImages==='function')await seiyaRefreshImages();}
$('checkRuntime').onclick=seiyaReviewConnection;$('key').onchange=seiyaReviewConnection;
globalThis.addEventListener?.('online',()=>{if(!seiyaHistoryAuthBlocked){refreshRuntimeStatus();refreshJobs();if(typeof seiyaRefreshImages==='function')seiyaRefreshImages();}});
async function seiyaDeleteHistoryChat(e){
 if(!seiyaJobHistoryEnabled&&typeof seiyaImageSession!=='function')return;
 e.preventDefault();e.stopImmediatePropagation();if(busy||seiyaHistoryDeleting)return;
 const chatId=activeId;seiyaHistoryDeleting=true;
 const controls=['send','clear','newMobile','chats','deleteChat'];controls.forEach(id=>$(id).disabled=true);
 try{
  if(storageBlocked||!saveChats())throw Error('No se puede guardar el historial; chat conservado.');
  const refs=[];
  if(seiyaJobHistoryEnabled)refs.push({control:jobControl,session:await jobSession(chatId)});
  if(typeof seiyaImageSession==='function'&&seiyaImagesConfigured)refs.push({control:seiyaImageRequest,session:await seiyaImageSession(chatId)});
  let pending=false;
   for(const ref of refs){const state=await ref.control({operation:'list',session:ref.session});seiyaValidJobList(state,ref.control===jobControl?64:8,ref.control===jobControl?jobLabels:seiyaImageLabels);ref.active=state.jobs.filter(j=>['queued','preparing','running','validating','generating','cancelling','quarantined'].includes(j.phase));pending||=ref.active.length>0;}
  if(pending){
   if(confirm('Hay trabajos activos o pendientes de revisión. ¿Solicitar cancelación sin borrar el chat?'))for(const ref of refs)for(const job of ref.active)if(job.phase!=='quarantined')await ref.control({operation:'cancel',session:ref.session,job_id:job.job_id});
   $('status').textContent='Chat conservado: espera confirmación de cancelación o revisión.';return;
  }
  if(!confirm('¿Eliminar esta conversación y sus trabajos e imágenes del historial local del host?'))return;
  for(const ref of refs)await ref.control({operation:'forget',session:ref.session});
  jobSessions.delete(chatId);jobRequests.delete(chatId);
  if(typeof seiyaForgetPreviews==='function'){seiyaForgetPreviews(chatId);seiyaImageSessions.delete(chatId);seiyaImagePending.delete(chatId);}
  seiyaHistoryDeleting=false;chats=chats.filter(c=>c.id!==chatId);if(chats.length)openChat(chats[0].id);else clearChat();
 }catch(error){$('status').textContent='Chat conservado: '+error.message;}
 finally{seiyaHistoryDeleting=false;controls.forEach(id=>$(id).disabled=busy);refreshJobs();if(typeof seiyaRefreshImages==='function')seiyaRefreshImages();}
}
$('deleteChat').addEventListener('click',seiyaDeleteHistoryChat,true);
