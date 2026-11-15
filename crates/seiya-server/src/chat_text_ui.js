// PreparedWeb text completion boundary. Never render an unvalidated response.
const seiyaTextFetch=seiyaChatFetch;
seiyaChatFetch=async function(headers,payload){
 let response;
 try{response=await seiyaTextFetch(headers,payload);}
 catch(error){
  if(error.confirmed_rejection)throw error;
  throw Error('No se confirmó la respuesta de chat. Estado incierto: comprueba el runtime antes de reenviar. '+error.message);
 }
 const data=await response.json();
 const answer=data?.choices?.[0]?.message;
 if(!Array.isArray(data?.choices)||data.choices.length!==1||answer?.role!=='assistant'||typeof answer.content!=='string'||!answer.content.trim()||(answer.tool_calls!=null&&(!Array.isArray(answer.tool_calls)||answer.tool_calls.length))||answer.function_call){
  throw Error('Respuesta de chat inválida o incompleta. No se confirmó ningún mensaje; comprueba el runtime antes de reenviar.');
 }
 // These fields are read by the retained renderer, after it mutates history.
 // Reject malformed optional state here, never after rendering the answer.
 if(data.web!=null&&(!data.web||typeof data.web!=='object'||Array.isArray(data.web)))throw Error('Fuentes de chat inválidas; no se confirmó la respuesta.');
 for(const key of ['sources','reads','searches'])if(data.web?.[key]!=null&&(!Array.isArray(data.web[key])||data.web[key].some(s=>!s||typeof s!=='object'||Array.isArray(s))))throw Error('Fuentes de chat inválidas; no se confirmó la respuesta.');
 if(data.jobs!=null)seiyaValidJobList(data.jobs,64,jobLabels);
 const seconds=data.swarm?.elapsed_seconds;
 const safe={...data,swarm:{...data.swarm,elapsed_seconds:typeof seconds==='number'&&Number.isFinite(seconds)&&seconds>=0?seconds:'—'}};
 return{ok:true,json:async()=>safe};
};

// A restored unanswered turn is not a new request. Recovery is explicit and
// only moves that last message back to the editor; it never dispatches work.
const seiyaRecoverText=document.createElement('button');
seiyaRecoverText.type='button';seiyaRecoverText.textContent='Recuperar mensaje sin respuesta';
seiyaRecoverText.hidden=true;$('form').append(seiyaRecoverText);
$('status').setAttribute('role','status');$('status').setAttribute('aria-live','polite');
function seiyaTextRecoveryState(){
 seiyaRecoverText.hidden=busy||seiyaHistoryDeleting||messages.at(-1)?.role!=='user';
}
seiyaRecoverText.onclick=()=>{
 if(busy||seiyaHistoryDeleting||messages.at(-1)?.role!=='user')return;
 const chat=activeChat(),last=messages.at(-1);
 if($('prompt').value&&$('prompt').value!==last.content){$('status').textContent='Conserva primero el borrador actual; no se sustituyó ni reenvió ningún mensaje.';return;}
 const previousDraft=chat.draft;const previousPrompt=$('prompt').value;
 messages.pop();chat.draft=last.content;$('prompt').value=last.content;
 if(storageBlocked||!saveChats()){
  messages.push(last);chat.draft=previousDraft;$('prompt').value=previousPrompt;
  $('status').textContent='No se pudo guardar la recuperación; mensaje y borrador conservados.';return;
 }
 openChat(chat.id);$('status').textContent='Mensaje recuperado como borrador, sin enviar. Si el envío anterior fue incierto, comprueba el runtime antes de reenviar.';$('prompt').focus();
};
const seiyaTextOpenChat=openChat;
openChat=function(id){seiyaTextOpenChat(id);seiyaTextRecoveryState();};
const seiyaTextSubmit=$('form').onsubmit;
$('form').onsubmit=async function(event){
 try{return await seiyaTextSubmit(event);}finally{seiyaTextRecoveryState();}
};
seiyaTextRecoveryState();
