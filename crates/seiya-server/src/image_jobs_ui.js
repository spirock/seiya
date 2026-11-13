// Optional image controls only. No new CSS/theme, model tool or direct runtime URL.
let seiyaImagesAvailable=false,seiyaImagesConfigured=false,seiyaImagesDurable=false,seiyaImageMode=false,seiyaImagesPolling=false;
const seiyaImageSessions=new Map(),seiyaImagePending=new Map(),seiyaImagePreviews=new Map();
const seiyaImageArtifacts=new Map(),seiyaImageEpoch=new Map();
const seiyaImageToggle=document.createElement('button');seiyaImageToggle.type='button';seiyaImageToggle.textContent='Imagen';seiyaImageToggle.hidden=true;seiyaImageToggle.setAttribute('aria-pressed','false');seiyaImageToggle.setAttribute('aria-label','Describir una imagen para generar');$('send').parentNode.append(seiyaImageToggle);
const seiyaImagePanel=document.createElement('section');seiyaImagePanel.hidden=true;seiyaImagePanel.setAttribute('aria-label','Imágenes locales');seiyaImagePanel.setAttribute('aria-live','polite');$('scroll').append(seiyaImagePanel);
function seiyaImageCapabilities(cap){seiyaImagesAvailable=cap?.available===true;seiyaImagesConfigured=cap?.configured===true||seiyaImagesAvailable;seiyaImagesDurable=cap?.volatile===false;seiyaImageToggle.hidden=!seiyaImagesAvailable&&!seiyaImageMode;seiyaImageToggle.disabled=busy;if(seiyaImagesConfigured)seiyaRefreshImages();}
function seiyaSetImageMode(value){seiyaImageMode=value;seiyaImageToggle.hidden=!seiyaImagesAvailable&&!value;seiyaImageToggle.setAttribute('aria-pressed',String(value));seiyaImageToggle.textContent=value?'Imagen ✓':'Imagen';$('prompt').placeholder=value?'Describe la imagen (512 × 512)…':'Pregunta a Pegassus…';}
seiyaImageToggle.onclick=()=>{if(!busy&&(seiyaImageMode||seiyaImagesAvailable)){seiyaSetImageMode(!seiyaImageMode);$('prompt').focus();}};
async function seiyaImageRequest(frame,binary=false){
 const work=async signal=>{
 const headers={'Content-Type':'application/json'};if($('key').value)headers.Authorization='Bearer '+$('key').value;
 const r=await fetch('/api/images/v1',{method:'POST',headers,signal,body:JSON.stringify(frame)});
 if(!r.ok){let data;try{data=await r.json();}catch{}if(typeof seiyaHttpError==='function')throw seiyaHttpError(r,data);const error=Error(runtimeError(data,r.statusText));error.confirmed_rejection=[400,401,403,404,409,413,415,429,503].includes(r.status);throw error;}
 if(!binary)return r.json();
 const length=Number(r.headers.get('content-length'));
 if(r.headers.get('content-type')!=='image/png'||!Number.isSafeInteger(length)||length<=0||length>16777216)throw Error('El servidor no devolvió un PNG acotado.');
 const blob=await r.blob();if(blob.size!==length||blob.type!=='image/png')throw Error('PNG incompleto; no se mostrará.');return blob;
 };return typeof seiyaControlFetch==='function'?seiyaControlFetch(work):work();
}
const seiyaImageOpenRequests=new Map();
async function seiyaImageSession(chatId){
 if(seiyaImageSessions.has(chatId))return seiyaImageSessions.get(chatId);
 if(!seiyaImageOpenRequests.has(chatId))seiyaImageOpenRequests.set(chatId,(async()=>{const reply=await seiyaImageRequest({operation:'open',conversation_id:chatId});if(typeof reply.session!=='string'||!/^[a-f0-9]{64}$/.test(reply.session))throw Error('Sesión de imágenes inválida.');seiyaImageSessions.set(chatId,reply.session);return reply.session;})().finally(()=>seiyaImageOpenRequests.delete(chatId)));
 return seiyaImageOpenRequests.get(chatId);
}
const seiyaImageLabels={queued:'En cola',preparing:'Comprobando permiso, recursos y archivos',running:'Ejecutando generador local',validating:'Validando PNG y finalización',cancelling:'Cancelación solicitada; esperando confirmación',completed:'PNG validado y ejecución finalizada',cancelled:'Cancelada',failed:'Falló; consulta el estado antes de reenviar',blocked:'Bloqueada por permisos o recursos',quarantined:'Finalización incierta; requiere revisión'};
function seiyaForgetPreviews(chatId){seiyaImageEpoch.set(chatId,(seiyaImageEpoch.get(chatId)||0)+1);for(const [key,url] of seiyaImagePreviews){if(key.startsWith(chatId+'/')){URL.revokeObjectURL(url);seiyaImagePreviews.delete(key);}}}
globalThis.addEventListener?.('pagehide',()=>{for(const chat of chats)seiyaForgetPreviews(chat.id);});
function seiyaRenderImages(data,chatId,session){
 if(chatId!==activeId)return;
 if(!Array.isArray(data.jobs)||data.jobs.length>8)throw Error('Estado de imágenes inválido.');
 if(typeof seiyaValidJobList==='function')seiyaValidJobList(data,8,seiyaImageLabels);
 seiyaImagePanel.hidden=false;seiyaImagePanel.replaceChildren();const title=document.createElement('h2');title.textContent='Imágenes locales';seiyaImagePanel.append(title);
 const note=document.createElement('p');note.textContent=seiyaImagesDurable?'Historial local persistente. Reiniciar no reenvía imágenes.':'Historial temporal del host. Recargar recupera enlaces sin reenviar; reiniciar el host puede perderlos.';seiyaImagePanel.append(note);
 for(const job of data.jobs){
  if(typeof job.job_id!=='string'||!seiyaImageLabels[job.phase])throw Error('Trabajo de imagen inválido.');
  if(seiyaImagePending.get(chatId)?.id===job.request_id||activeChat()?.imagePending?.id===job.request_id){seiyaImagePending.delete(chatId);delete activeChat().imagePending;saveChats();}
  const row=document.createElement('div');row.setAttribute('data-image-job-id',job.job_id);const status=document.createElement('p');status.textContent=job.job_id+' · '+seiyaImageLabels[job.phase];row.append(status);
  if(['queued','preparing','running','validating','cancelling'].includes(job.phase)){
   const cancel=document.createElement('button');cancel.type='button';cancel.textContent='Cancelar imagen';cancel.disabled=job.phase==='cancelling';cancel.onclick=async()=>{cancel.disabled=true;try{await seiyaImageRequest({operation:'cancel',session,job_id:job.job_id});await seiyaRefreshImages();}catch(e){status.textContent=e.message;cancel.disabled=false;}};row.append(cancel);
  }
  if(job.phase==='completed'&&job.artifact_available===true){
   const key=chatId+'/'+job.job_id;const show=()=>{const url=seiyaImagePreviews.get(key);if(!url)return;const img=document.createElement('img');img.src=url;img.alt='Imagen generada y validada por Seiya';img.width=512;img.style.maxWidth='100%';row.append(img);};
   const artifact=async()=>{
    if(seiyaImagePreviews.has(key))return seiyaImagePreviews.get(key);
    if(!seiyaImageArtifacts.has(key)){const epoch=seiyaImageEpoch.get(chatId)||0;const request=(async()=>{const blob=await seiyaImageRequest({operation:'artifact',session,job_id:job.job_id},true);if(!chats.some(c=>c.id===chatId)||(seiyaImageEpoch.get(chatId)||0)!==epoch)throw Error('La conversación se eliminó; no se conservará el PNG.');const url=URL.createObjectURL(blob);seiyaImagePreviews.set(key,url);return url;})().finally(()=>seiyaImageArtifacts.delete(key));seiyaImageArtifacts.set(key,request);}
    return seiyaImageArtifacts.get(key);
   };
   const view=document.createElement('button');view.type='button';view.textContent='Ver imagen';view.onclick=async()=>{if(view.disabled)return;view.disabled=true;try{await artifact();if(activeId===chatId)show();}catch(e){status.textContent=e.message;view.disabled=false;}};row.append(view);
   const download=document.createElement('button');download.type='button';download.textContent='Descargar PNG';download.onclick=async()=>{if(download.disabled)return;download.disabled=true;try{const url=await artifact();if(activeId!==chatId)throw Error('La conversación cambió; descarga detenida.');const a=document.createElement('a');a.href=url;a.download='seiya-image.png';row.append(a);a.click();a.remove();}catch(e){status.textContent=e.message;}finally{download.disabled=false;}};row.append(download);
   if(seiyaImagePreviews.has(key)){view.disabled=true;show();}
  }
  seiyaImagePanel.append(row);
 }
 if(!data.jobs.length){const empty=document.createElement('p');empty.textContent='Sin imágenes enlazadas a esta conversación.';seiyaImagePanel.append(empty);}
}
async function seiyaRefreshImages(){
 if(seiyaImagesPolling||typeof seiyaHistoryDeleting!=='undefined'&&seiyaHistoryDeleting||typeof seiyaHistoryAuthBlocked!=='undefined'&&seiyaHistoryAuthBlocked)return;const chatId=activeId;
 if(!chatId||!seiyaImagesConfigured&&!seiyaImageSessions.has(chatId)){seiyaImagePanel.hidden=true;return;}seiyaImagesPolling=true;
 try{const session=await seiyaImageSession(chatId);seiyaRenderImages(await seiyaImageRequest({operation:'list',session}),chatId,session);}catch(e){if(chatId===activeId){seiyaImagePanel.hidden=false;seiyaImagePanel.textContent=e.message+' No se reenviará automáticamente.';seiyaImageSessions.delete(chatId);}}finally{seiyaImagesPolling=false;}
}
setInterval(seiyaRefreshImages,2000);
$('form').addEventListener('submit',async e=>{
 if(!seiyaImageMode&&activeChat()?.imagePending){e.preventDefault();e.stopImmediatePropagation();seiyaSetImageMode(true);$('status').textContent='Hay una imagen sin confirmar. Consulta el panel antes de reenviar; no se envió al chat.';return;}
 if(!seiyaImageMode)return;e.preventDefault();e.stopImmediatePropagation();if(busy)return;if(!seiyaImagesAvailable){$('status').textContent='Imágenes no disponibles. No se ha enviado nada; pulsa Imagen para volver al chat.';return;}
 const composerValue=$('prompt').value,text=composerValue.trim();if(!text)return;if(new TextEncoder().encode(text).length>2048){$('status').textContent='La descripción supera 2048 bytes; no se ha enviado.';return;}if(text.includes('\0')){$('status').textContent='La descripción contiene un carácter NUL; no se ha enviado.';return;}
 const chat=activeChat();let pending=seiyaImagePending.get(chat.id);
 if(pending&&pending.prompt!==text){$('status').textContent='Hay un envío de imagen sin confirmar. Consulta su estado antes de cambiar la descripción.';return;}
 if(!pending&&chat.imagePending){pending=chat.imagePending;seiyaImagePending.set(chat.id,pending);if(pending.prompt!==text){$('status').textContent='Hay un envío de imagen sin confirmar. Consulta su estado antes de cambiar la descripción.';return;}}
 if(!pending){pending={id:crypto.randomUUID(),prompt:text};seiyaImagePending.set(chat.id,pending);chat.imagePending=pending;if(!saveChats()){seiyaImagePending.delete(chat.id);delete chat.imagePending;$('status').textContent='No se envió la imagen: no se pudo guardar su referencia de recuperación.';return;}}
 if(typeof pending.id!=='string'||typeof pending.prompt!=='string'||typeof seiyaUuid==='function'&&!seiyaUuid(pending.id)){$('status').textContent='Referencia de imagen inválida; no se ha enviado.';return;}
 busy=true;['send','clear','newMobile','chats','deleteChat'].forEach(id=>$(id).disabled=true);seiyaImageToggle.disabled=true;
 try{
  const session=await seiyaImageSession(chat.id);const submitted=await seiyaImageRequest({operation:'submit',session,request_id:pending.id,request:{prompt:text,width:512,height:512,seed:42}});
  if(!submitted||typeof submitted.job_id!=='string'||typeof seiyaUuid==='function'&&!seiyaUuid(submitted.job_id)||submitted.request_id!==pending.id||!Object.hasOwn(seiyaImageLabels,submitted.phase))throw Error('Registro de imagen no confirmado; consulta el panel antes de reenviar.');
  seiyaImagePending.delete(chat.id);delete chat.imagePending;if(activeId===chat.id&&$('prompt').value===composerValue){chat.draft='';$('prompt').value='';seiyaSetImageMode(false);}saveChats();$('status').textContent='Solicitud de imagen registrada. El panel mostrará su estado real.';await seiyaRefreshImages();
 }catch(error){if(error.confirmed_rejection){seiyaImagePending.delete(chat.id);delete chat.imagePending;saveChats();}$('status').textContent=error.message+' No se reenviará automáticamente; consulta el panel.';await seiyaRefreshImages();}
 finally{busy=false;['send','clear','newMobile','chats','deleteChat'].forEach(id=>$(id).disabled=false);seiyaImageToggle.disabled=false;updatePicker();$('prompt').focus();}
},true);
const seiyaOriginalDelete=$('deleteChat').onclick;
$('deleteChat').addEventListener('click',async e=>{
 if(typeof seiyaDeleteHistoryChat==='function')return;
 const chatId=activeId,session=seiyaImageSessions.get(chatId);if(!session||busy)return;e.preventDefault();e.stopImmediatePropagation();
 try{
  const state=await seiyaImageRequest({operation:'list',session});if(!Array.isArray(state.jobs))throw Error('Estado de imágenes inválido.');
  const pending=state.jobs.filter(j=>['queued','preparing','running','validating','cancelling','quarantined'].includes(j.phase));
  if(pending.length){
   if(!confirm('Hay imágenes activas o pendientes de revisión. ¿Solicitar cancelación sin borrar aún el chat?'))return;
   for(const job of pending)if(job.phase!=='quarantined')await seiyaImageRequest({operation:'cancel',session,job_id:job.job_id});
   $('status').textContent='Chat conservado: espera confirmación de cancelación o revisión de las imágenes.';await seiyaRefreshImages();return;
  }
  await seiyaOriginalDelete.call($('deleteChat'),e);
  if(!chats.some(c=>c.id===chatId)){seiyaImageSessions.delete(chatId);seiyaImagePending.delete(chatId);seiyaForgetPreviews(chatId);await seiyaRefreshImages();}
 }catch(error){$('status').textContent='Chat conservado: '+error.message;}
},true);
