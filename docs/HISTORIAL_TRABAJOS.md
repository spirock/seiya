# Historial de trabajos e imágenes · offline opt-in

ADR0052; plan `plans/2026-11-11-historial-y-robustez.md`.
Lógica compartida Rust: `job_store.rs`, `jobs.rs` e `image_jobs.rs`. No modelos
descargados/cargados, generación real o activación de servicios por este incremento.

## Qué se conserva

Borradores de texto, estados, UUID de petición/conversación y PNG validados del
broker, en directorios privados separados para drafts/images. El historial de
conversación sigue en localStorage del navegador existente; no se sincroniza a
otro PC ni se migra automáticamente al servidor. No guardamos claves API en
snapshots; contienen prompts y capacidades del broker, son sensibles y no cifrados.
La UI solo conserva UUID/referencia de envío ambiguo dentro del chat; no PNG,
capacidad del broker, contraseña o URL Blob en localStorage.
No resucita trabajos perdidos de un host anterior volátil ni importa la vista
previa simulada como resultados reales. No se migra evidencia W5 como generación.

`Jobs::persistent`, `ImageJobs::persistent` y `PreparedWeb::with_history` son
opt-in. Los constructores anteriores siguen volátiles. `seiya-image-web` requiere
ahora `--history-dir DIRECTORIO_PRIVADO_ESTABLE`, además de flags ADR0051; crea
subdirectorios images/drafts privados. Debe ser absoluto, canónico,0700, existente
y reutilizarse en el próximo arranque (no una raíz temporal nueva). No ejecutar
el host por tener un almacén: activación real sigue bloqueada por critical.
Guardar bajo el temporal del SO no garantiza supervivencia a su limpieza/reboot;
el operador debe elegir una ubicación privada estable autorizada. No se instala
ni crea un almacén de datos personales fuera de las fixtures durante los tests.

## Atomicidad, cuotas y fallos

Un propietario por directorio: owner.lock con lock del kernel durante toda la
instancia. No borrar/renombrar el lock para permitir un segundo escritor. Snapshot
versionado, cabecera JSON≤6MiB y blobs binarios≤32MiB/8, cada uno≤16MiB. Límites
de brokers originales:16sesiones/8imágenes y32sesiones/64borradores≤64KiB.
Snapshots0600; temp exclusivo, sync_all, rename, sync directorio. Se comprueban
tipo/tamaño/permisos/symlinks. No robustez prometida frente a cambios de otro
proceso del mismo usuario con acceso al directorio privado o filesystem hostil.
No cifrado, GC/purga automática, journal infinito o garantía de rendimiento; cada
mutación relevante reescribe el snapshot acotado. Consultar list/status/artifact
no reescribe PNG. Temporales de un crash pueden quedar para revisión del operador.

Se guarda antes de aceptar submit, antes de ejecutar y antes de publicar resultado.
Si falla disco/checkpoint, se cierra admisión y se pide cancelar trabajo activo;
error visible, sin respuesta de éxito/PNG ni reintentos automáticos. El snapshot
previo se conserva. Si hay error de lectura/corrupción/versión no soportada,
constructor falla sin sobreescribirlo como vacío. Un error después de rename/sync
puede haber publicado snapshot nuevo: no se promete rollback de una escritura
incierta. La recuperación es explícita con evidencia, no borrar datos para eludirla.

Reinicio no reejecuta trabajos: queued se restaura cancelled; cualquier fase activa
se vuelve quarantined y cierra pool. No recupera PIDs/grants ni mata procesos del
snapshot. Deben revisarse runtime/lease/marcadores nativos por el operador antes
de otro perfil. Terminal completed mantiene artefacto; PNG se decodifica y compara
metadata al abrir, un blob corrupto impide abrir el almacén. No garantiza calidad
visual/fidelidad al prompt. Artefactos antiguos no se ofrecen como nuevas imágenes.

## Recuperación y borrado desde UI

Extensión aditiva a controles browser: `open` con UUID de conversación y `forget`
con sesión. Auth/Host/Origin antes de leer datos: perfil de **un propietario**,
no multiusuario; quien comparte la misma autenticación tiene el mismo ámbito.
Open renueva TTL y vuelve a enlazar sin submit ni permisos de inferencia. Con
host volátil recupera refresco, no reinicio. Cookie de acceso temporal puede
expirar: se necesita autenticar de nuevo; snapshots no renuevan credenciales.

Al borrar chat se comprueban ambos brokers. Trabajos activos solicitan cancelación
y conservan el chat hasta confirmación; cuarentena exige revisión. Confirmación
explícita borra registros/artefactos propios, libera cuota y revoca URLs Blob.
No borra pesos, evidencia privada del runtime, documentos originales o backups.
No transacción distribuida entre dos brokers y localStorage: un fallo intermedio
puede borrar un broker y conservar el chat/otro broker; se informa sin inventar
rollback y se permite consultar/repetir borrado explícito. No borrado forense.

## Validación

Fixtures propias, ejecutores simulados, PNG sintéticos y DOM/red simulados. No
modelos reales ni navegador gráfico. Evidencia/comandos finales en el plan. Para
rollback omitir constructores durables y conservar directorios; no existe reset
HTTP de cuarentena ni borrado silencioso de snapshots incompatibles.

## Reconexión, cancelación y errores (punto4)

PreparedWeb incorpora un adaptador JS opcional de controles sin cambiar CSS/layout
o el frontend del servicio legado. Consultas/control/descargas/health con plazo
15s, chat150s (por encima del runtime120s); se acota también lectura de cuerpo.
Aborto de fetch o timeout **no acredita fin/cancelación del runtime**: conserva
referencia ambigua y exige consultar antes de reenviar. Sin submit automático
al reconectar, refrescar, pulsar Revisar conexión o volver online.

401/403 pausa polling hasta autenticar y pulsar Revisar conexión/cambiar clave;
no renueva credenciales. TTL/404 o fallo de transporte descartan la capacidad
cacheada y Open/List pueden recuperar la misma conversación en próxima consulta.
Estado desconocido/inconsistente, JSON inválido o PNG incompleto no se interpreta
como resultado completado ni habilita borrar un chat incierto. Reintento de un
submit ambiguo es solo manual y conserva UUID; fallo de almacenamiento browser
impide enviar una imagen sin referencia durable. Un pending recuperado nunca se
manda silenciosamente como texto al chat.

Controles de cambio/borrado de chat se bloquean durante registro; compositor
sigue editable. ACK tardío no borra un nuevo borrador ni cambia su modalidad a
chat. Lista tardía de otro chat no repinta el actual. View/download comparten
transferencia PNG, evitan doble clic y descartan Blob si el chat se borró mientras
se descargaba. Se revocan URLs al borrar/pagehide, no se guardan en historial.
Una descarga de borrador valida texto≤64KiB y revoca su URL tras uso. Cancelar es
idempotente; la UI espera estado terminal y Forget rechaza tareas que aún viven
incluso si figuraban cancelled al salir de cola. Cuarentena nunca se borra por UI.

Los tests de reinicio reabren constructores/snapshots propios en el mismo host y
simulan el estado interrumpido; no son ensayo de corte eléctrico, filesystem
remoto, crash del SO, otro PC o sesión larga con modelos. Directorio fsync/lock y
decoder macOS se ejercitan realmente con archivos/PNG sintéticos. Modelos y red
de controles/DOM están simulados, no equivalen a validación de la web desplegada.
