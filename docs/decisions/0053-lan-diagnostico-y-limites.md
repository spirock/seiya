# ADR0053 · prueba LAN sin inferencia y configuración/diagnóstico

Alcance: LAN/HTTPS/autenticación sin modelos y, después, límites ajustables y
causa de bloqueo visibles, sin rediseño. La
activación real ADR0051 sigue bloqueada; no probar Gemma/imágenes ni reiniciar
servicios compartidos para acreditar conectividad.

## LAN de diagnóstico

Host Rust separado `seiya-web-diagnostics`, misma web con chat explícitamente
deshabilitado y sin ejecutores reales/broker de imagen adjuntos. IP privada
concreta:8016, subred IPv4 explícita, TLS1.3/certificado local, Host/Origin exactos
y cookies Secure/HttpOnly/SameSite. Vida≤30min, sin abrir Internet/firewall/UPnP.
Reutiliza autenticación y listener TLS existentes; no proxy ni segundo runtime.
Dos bootstrap de un uso: uno privado para autocheck local, otro para el usuario.
No consumir el enlace del usuario al verificar. El acceso desde otro PC solo
puede confirmarlo el usuario: no inventar evidencia remota desde el propio Mac.

## Límites y observaciones

API browser autenticada separada `/api/diagnostics/v1`, comandos cerrados status
y configure con revisión CAS, JSON≤4KiB. Cambios son configuración de propietario,
nunca grants de inferencia, reset de cuarentena, descarga o shell. Rechazar campos
extra, valores fuera de rangos y conflictos; persistir snapshot privado atómico
antes de confirmar. Sin claves API ni rutas introducidas por el navegador.
Config por defecto conserva ADR0051: Gemma warning≤10000ms/swap≤2048MiB/
deadline≤120s, imagen warning0/swap≤512MiB/deadline≤600s, sesión≤1800s. Se puede
reducir dentro de rangos cerrados, no superar techos o tolerar critical/unknown.
Duración sesión se aplica al siguiente arranque; límites por ejecución se capturan
al comenzar un turno, nunca se relajan a mitad de ejecución. No activar modelos
al guardar. Alternativa descartada: un slider sin contrato efectivo en Rust.

Diagnóstico distingue observación read-only del host, límites configurados,
estado de admisión y último bloqueo registrado. Memoria actual no acredita
capacidad/residencia ni invalida el critical previo. Importación de stop previo
solo desde ruta privada explícita del operador, tamaño/schema acotados, sin
devolver rutas/PIDs/logs/prompts al navegador. Registrar futuros stops desde host
con muestra y condición; conservar cierre/lease/cuarentena existentes.
No leer documentos personales ni listar procesos ajenos para el panel.

Rollback: omitir diagnósticos y constructores nuevos; conservar configuraciones/
evidencia. No cambiar defaults globales de otros perfiles o wire OpenAI/nativo.
