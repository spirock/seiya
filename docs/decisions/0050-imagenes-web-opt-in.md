# ADR0050 · conectar imágenes a la web sin activar inferencia

Alcance: petición, estado, cancelación y descarga
local desde la misma interfaz, sin rediseño. Lógica nueva Rust y frontend JS
mínimo en HTML existente. No instalación, servidor nuevo, otra imagen
real o cambio del perfil activo. Permiso W5 de imagen única consumido; imágenes
web quedan apagadas por defecto hasta autorización/host efectivos.

## Contrato y responsabilidades

Broker de imágenes separado de jobs-v1 de texto y de OpenAI/SessionRequest.
API browser interna `/api/images/v1`, auth/Host/Origin antes de cuerpos acotados.
Comandos cerrados sesión/submit/list/status/cancel/artifact, UUID idempotente por
envío y capacidad opaca por conversación en memoria, nunca en prompts, URL o
localStorage. Request textual≤2048bytes, inicialmente512×512 y seed u32; sin
rutas/URL/modelos/binarios elegidos por navegador o modelo.

Un ejecutor original propietario,1 activo/2 esperas,8 registros máximos,32MiB
totales de PNG conservados y16MiB por PNG. Estado observable sin porcentajes:
queued/preparing/running/validating/cancelling y terminales. Cancelación es una
petición, no cleanup: no publicar PNG ni liberar slot como seguro hasta que el
ejecutor confirme fin. Deadline total620s, petición de cancelación y gracia10s;
timeout ambiguo/drop/panic→cuarentena del pool, sin reset/retry HTTP. Sesiones
volátiles30min renovadas por controles, no persistencia/replay tras refrescar.

El broker decodifica PNG/CRC/zlib/filtros/dimensiones/hash antes de completion;
el ejecutor solo devuelve bytes después de cleanup confirmado. Artifact se sirve
como image/png autenticado, nombre fijo/attachment/nosniff/no-store, no SVG/HTML,
base64 gigante o rutas de disco. Resultados no acreditan calidad semántica.
Primer adaptador conserva criterio conservador de imagen visible/no uniforme.

## Adaptador nativo y permisos

Reutilizar assets/hash, argv fijo sd.cpp, sandbox, lease y OwnedChild Rust.
Prompt/seed son argumentos aislados, no shell; parámetros Turbo8/CFG1/Euler
fijados por host. Preparación/hash cancelables entre bloques, antes/durante
spawn comprobación de permiso del host. Lease existente impide coexistencia con
Gemma privado residente;11434 debe estar vacío, sin adopción/unload/kill ajenos.
Memoria normal y swap+512MiB por defecto en este ejecutor nuevo, sin heredar
excepción informativa W5. Deadline600s, disco/log, cleanup y cuarentena persistente
si una future se pierde después de spawn. No cargar para anunciar capacidad.

Callbacks de admisión/permiso son código confiable del host, no booleanos JSON.
Default denegado; adjuntar broker al router es opt-in del constructor y no lo
activa en CLIs existentes. Host debe resolver recursos/ciclo del chat antes de
admitir imágenes. No liberar ni descargar Gemma por iniciativa de este incremento.

## UI y alternativas

Reutilizar compositor, botones/estilos y panel con un control Imagen opcional;
mostrarlo solo con capacidad efectiva. Solicitud explícita del usuario, sin
tool conversacional nueva o detección por palabras. Estado/cancelación/PNG por
capacidad del chat; render texto seguro y Blob PNG autenticado, no URL del modelo.
Refrescar no reenvía imagen; borrado de chat con imagen activa pide cancelar y
espera confirmación. Controles de status/cancel/descarga disponibles aun revocada
admisión. El HTML/CSS original no se sustituye: inyección acotada solo en perfil
con imágenes configurado. Conversación con Gemma no promete concurrencia GPU.

Alternativas descartadas: meter PNG en broker de borradores, prosa que diga «ya
generé», API remota, shells/modelos descargados y desactivar límites para coexistir
con chat. Rollback: no adjuntar ImageJobs al router; app/demo/historial/configs
anteriores intactos y evidencia privada conservada. Pruebas con PNG sintéticos,
ejecutores simulados y HTTP en proceso; nueva inferencia real requiere permiso.
