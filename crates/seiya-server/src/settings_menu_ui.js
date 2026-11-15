// Same chat surface, compiled setup controls only. No frame/eval/remote HTML.
const seiyaModelsBase=(()=>{
 try{if(typeof seiyaModelsHome!=='string')return null;const url=new URL(seiyaModelsHome),current=new URL(location.href);
  if(url.origin!==current.origin||url.protocol!=='http:'||url.hostname!=='127.0.0.1'||url.port!=='8015'||url.username||url.password||url.search||url.hash||!/^\/app\/[a-f0-9]{32}\/$/i.test(url.pathname))return null;
  return url;
 }catch{return null;}
})();
const seiyaSettingsRoot=typeof seiyaConfigurationButton==='undefined'?document.createElement('button'):seiyaConfigurationButton;
if(typeof seiyaConfigurationButton==='undefined'){seiyaSettingsRoot.type='button';seiyaSettingsRoot.textContent='Configuración';(document.querySelector?.('aside')||$('scroll')).append(seiyaSettingsRoot);}
const seiyaSettingsMenu=document.createElement('nav');seiyaSettingsMenu.id='seiyaSettingsMenu';seiyaSettingsMenu.hidden=true;seiyaSettingsMenu.setAttribute('aria-label','Opciones de configuración');
seiyaSettingsRoot.setAttribute('aria-controls',seiyaSettingsMenu.id);seiyaSettingsRoot.setAttribute('aria-expanded','false');
seiyaSettingsRoot.insertAdjacentElement('afterend',seiyaSettingsMenu);
const seiyaGeneralSettings=typeof seiyaDiagnosticsPanel==='undefined'?document.createElement('section'):seiyaDiagnosticsPanel;
seiyaGeneralSettings.id='seiyaGeneralSettings';seiyaGeneralSettings.hidden=true;
if(typeof seiyaDiagnosticsPanel==='undefined'){seiyaGeneralSettings.textContent='La configuración general no está disponible en este host.';$('scroll').append(seiyaGeneralSettings);}
else seiyaDiagnosticsTitle.textContent='Configuración general';
const seiyaModelSettings=document.getElementById('setup');
if(seiyaModelSettings){seiyaModelSettings.hidden=true;Object.assign(seiyaModelSettings.style,{position:'static',inset:'auto',width:'auto',maxWidth:'780px',margin:'0 auto',overflow:'visible'});$('scroll').append(seiyaModelSettings);}
document.getElementById('seiyaModelsSource')?.remove();
const seiyaChatSurface=document.createElement('div');seiyaChatSurface.id='seiyaChatSurface';
for(const node of [...$('scroll').children])if(node!==seiyaGeneralSettings&&node!==seiyaModelSettings&&node!==seiyaSettingsRoot&&node!==seiyaSettingsMenu)seiyaChatSurface.append(node);
$('scroll').append(seiyaChatSurface);
const seiyaSettingsBack=document.createElement('button');seiyaSettingsBack.type='button';seiyaSettingsBack.textContent='Volver al chat';seiyaSettingsBack.hidden=true;$('scroll').prepend(seiyaSettingsBack);
const seiyaSettingsComposer=$('form').parentNode;
let seiyaSettingsView='chat',seiyaSettingsScroll=0,seiyaSettingsComposerHidden=false;
function seiyaSettingsExpanded(value){seiyaSettingsMenu.hidden=!value;seiyaSettingsRoot.setAttribute('aria-expanded',String(value));}
function seiyaCloseSettings(){
 if(seiyaSettingsView!=='chat'){
  seiyaChatSurface.hidden=false;seiyaGeneralSettings.hidden=true;if(seiyaModelSettings)seiyaModelSettings.hidden=true;seiyaSettingsBack.hidden=true;
  if(seiyaSettingsComposer)seiyaSettingsComposer.hidden=seiyaSettingsComposerHidden;
  $('scroll').scrollTop=seiyaSettingsScroll;seiyaSettingsView='chat';
 }
 seiyaSettingsExpanded(false);seiyaSettingsSelection();
}
function seiyaSettingsSelection(){for(const [view,button]of seiyaSettingsOptions)button.setAttribute('aria-pressed',String(seiyaSettingsView===view));}
function seiyaOpenSettings(view){
 if(busy||typeof seiyaHistoryDeleting!=='undefined'&&seiyaHistoryDeleting){$('status').textContent='Espera a que termine la operación del chat antes de abrir la configuración.';return;}
 if(view==='models'&&(!seiyaModelsBase||!seiyaModelSettings)){$('status').textContent='Configuración de modelos no disponible en este host.';return;}
 if(seiyaSettingsView==='chat'){seiyaSettingsScroll=$('scroll').scrollTop;seiyaSettingsComposerHidden=Boolean(seiyaSettingsComposer?.hidden);}
 seiyaChatSurface.hidden=true;seiyaGeneralSettings.hidden=view!=='general';if(seiyaModelSettings)seiyaModelSettings.hidden=view!=='models';
 seiyaSettingsBack.hidden=false;if(seiyaSettingsComposer)seiyaSettingsComposer.hidden=true;seiyaSettingsView=view;seiyaSettingsExpanded(true);seiyaSettingsSelection();$('scroll').scrollTop=0;
 if(view==='models')seiyaInitializeModels();
 const panel=view==='models'?seiyaModelSettings:seiyaGeneralSettings;panel.setAttribute('tabindex','-1');panel.focus?.({preventScroll:true});
}
const seiyaSettingsOptions=new Map();
for(const [view,label,target]of [['models','Configuración de modelos','setup'],['general','Configuración general',seiyaGeneralSettings.id]]){
 const button=document.createElement('button');button.type='button';button.textContent=label;button.style.display='block';if(view!=='models'||seiyaModelSettings)button.setAttribute('aria-controls',target);button.setAttribute('aria-pressed','false');button.onclick=()=>seiyaOpenSettings(view);
 if(view==='models'&&(!seiyaModelsBase||!seiyaModelSettings)){button.disabled=true;button.title='Este host no dispone de un gestor de modelos autenticado.';}
 seiyaSettingsOptions.set(view,button);seiyaSettingsMenu.append(button);
}
seiyaSettingsRoot.onclick=()=>seiyaSettingsExpanded(seiyaSettingsMenu.hidden);
seiyaSettingsBack.onclick=()=>{seiyaCloseSettings();if(!busy)$('prompt').focus();};
seiyaSettingsMenu.addEventListener('keydown',event=>{if(event.key==='Escape'){event.preventDefault();seiyaCloseSettings();seiyaSettingsRoot.focus();}});
for(const panel of [seiyaGeneralSettings,seiyaModelSettings])panel?.addEventListener('keydown',event=>{if(event.key==='Escape'){event.preventDefault();seiyaCloseSettings();seiyaSettingsRoot.focus();}});
const seiyaSettingsOpenChat=openChat;
openChat=function(id){if(!busy&&!(typeof seiyaHistoryDeleting!=='undefined'&&seiyaHistoryDeleting))seiyaCloseSettings();seiyaSettingsOpenChat(id);};
if(typeof seiyaShowDiagnostics==='function'){
 const previous=seiyaShowDiagnostics;seiyaShowDiagnostics=function(data){previous(data);seiyaGeneralSettings.hidden=seiyaSettingsView!=='general';seiyaSettingsRoot.setAttribute('aria-expanded',String(!seiyaSettingsMenu.hidden));};
}
const seiyaModelsLink=document.getElementById('seiyaModelsSettingsLink');
if(seiyaModelsLink)seiyaModelsLink.onclick=event=>{event.preventDefault();seiyaOpenSettings('models');};
