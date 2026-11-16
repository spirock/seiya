// Explicit chat-image increment. Reuse the frozen composer and image panel.
// No model/runtime calls on load, no replay after reload, no untrusted HTML.
const seiyaChatWithoutImages=seiyaChatFetch;
function seiyaChatImageDeletionBlocked(){
 if(activeChat()?.chatImagePending===undefined)return false;
 $('status').textContent='Chat conservado: hay un envío sin confirmar. Consulta el panel y recupera el registro antes de eliminar.';return true;
}
$('deleteChat').addEventListener('click',event=>{
  if(seiyaChatImageDeletionBlocked()){event.preventDefault();event.stopImmediatePropagation();}
},true);
function seiyaChatImagePendingValid(pending){return pending&&typeof pending==='object'&&seiyaUuid(pending.id)&&typeof pending.text==='string'&&pending.text.trim().length>0;}
seiyaChatFetch=async function(headers,payload){
 const chat=activeChat();if(!chat||typeof chat.id!=='string')throw Error('Conversación no disponible; no se envió nada.');
 const text=payload.messages?.at(-1)?.content;
 if(typeof text!=='string')throw Error('Mensaje inválido; no se envió nada.');
 let pending=chat.chatImagePending;
  if(pending!==undefined&&(!seiyaChatImagePendingValid(pending)||pending.text!==text))throw Error('Hay un envío de chat/imagen sin confirmar. Consulta el panel y conserva el mensaje antes de reenviar.');
 const session=await seiyaImageSession(chat.id);
 if(!pending){pending={id:crypto.randomUUID(),text};chat.chatImagePending=pending;if(!saveChats()){delete chat.chatImagePending;throw Error('No se pudo guardar la referencia; no se envió el chat.');}}
 const response=await seiyaChatWithoutImages(headers,{...payload,image_session:session,image_request_id:pending.id});
 const data=await response.json();
  if(Object.hasOwn(data,'image_job')){
   if(!data.image_job||typeof data.image_job!=='object'||data.image_job.request_id!==pending.id)throw Error('Registro de imagen inválido. Consulta el panel antes de reenviar.');
   seiyaValidJobList({jobs:[data.image_job]},8,seiyaImageLabels);
  }
   const previousBindings=chat.imageJobMessages;
   if(data.image_job&&Array.isArray(chat.messages))chat.imageJobMessages={...previousBindings,[pending.id]:chat.messages.length};
   delete chat.chatImagePending;if(!saveChats()){chat.chatImagePending=pending;chat.imageJobMessages=previousBindings;throw Error('Registro recibido pero no guardado. Conserva la referencia y recupera desde el panel antes de reenviar.');}
  seiyaChatImageRecoveryState();
 if(data.image_job)await seiyaRefreshImages();
 return {ok:true,json:async()=>data};
};

// Observing a matching broker job is evidence of registration, not completion.
// Confirming recovery is explicit and rechecks a fresh list, never resubmits.
const seiyaRecoverChatImage=document.createElement('button');
seiyaRecoverChatImage.type='button';seiyaRecoverChatImage.textContent='Recuperar imagen registrada';
seiyaRecoverChatImage.hidden=true;$('form').append(seiyaRecoverChatImage);
let seiyaChatImageObserved=null;
function seiyaChatImageRecoveryState(){
 const pending=activeChat()?.chatImagePending;
 seiyaRecoverChatImage.hidden=!seiyaChatImagePendingValid(pending)||seiyaChatImageObserved?.chatId!==activeId||seiyaChatImageObserved?.requestId!==pending.id;
}
function seiyaChatImageMatchingJob(data,pending){
 seiyaValidJobList(data,8,seiyaImageLabels);
 const matches=data.jobs.filter(job=>job.request_id===pending.id);
 if(matches.length>1)throw Error('Registro de imagen ambiguo; referencia conservada.');
 return matches[0];
}
const seiyaChatRenderImages=seiyaRenderImages;
seiyaRenderImages=function(data,chatId,session){
 seiyaChatRenderImages(data,chatId,session);
 if(chatId!==activeId)return;
 seiyaChatImageObserved=null;
 const pending=activeChat()?.chatImagePending;
 try{if(seiyaChatImagePendingValid(pending)&&seiyaChatImageMatchingJob(data,pending))seiyaChatImageObserved={chatId,requestId:pending.id};}
 finally{seiyaChatImageRecoveryState();}
};
const seiyaChatImageOpenChat=openChat;
openChat=function(id){seiyaChatImageOpenChat(id);if(activeId===id){seiyaChatImageObserved=null;seiyaChatImageRecoveryState();}};
seiyaRecoverChatImage.onclick=async()=>{
 if(busy||seiyaHistoryDeleting||seiyaRecoverChatImage.disabled)return;
 const chat=activeChat(),pending=chat?.chatImagePending;
 if(!seiyaChatImagePendingValid(pending))return;
 seiyaRecoverChatImage.disabled=true;
 try{
  const session=await seiyaImageSession(chat.id),data=await seiyaImageRequest({operation:'list',session});
  if(activeId!==chat.id||busy||seiyaHistoryDeleting||chat.chatImagePending!==pending)return;
  const job=seiyaChatImageMatchingJob(data,pending);
  if(!job)throw Error('El registro no está confirmado en el panel; referencia conservada, sin reenvío.');
  const previousDraft=chat.draft,previousPrompt=$('prompt').value;
  if(previousPrompt===pending.text)$('prompt').value='';
  chat.draft=$('prompt').value;delete chat.chatImagePending;
  if(storageBlocked||!saveChats()){chat.chatImagePending=pending;chat.draft=previousDraft;$('prompt').value=previousPrompt;throw Error('No se pudo guardar la recuperación; referencia y borrador conservados.');}
  seiyaChatImageObserved=null;seiyaChatImageRecoveryState();
  seiyaRenderImages(data,chat.id,session);
  $('status').textContent='Registro de imagen recuperado sin reenviar. Estado observado: '+seiyaImageLabels[job.phase]+'. Consulta el panel; recuperar no vuelve a generar la imagen.';
 }catch(error){$('status').textContent=error.message;}
 finally{seiyaRecoverChatImage.disabled=false;}
};
