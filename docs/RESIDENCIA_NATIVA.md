# Residencia gestionada nativa · macOS experimental

Implementación experimental; verificar el SHA de las fuentes y BUILD-INFO.json
del paquete descargado. Driver
Rust común para cerebro, trabajador e imagen: preparación → carga → ejecución
→ descarga/cierre → inactivo confirmado, o cuarentena. Validado con fixtures y
proceso auxiliar propio; **no validado con modelos, VRAM o sesiones reales**.

## Qué hace

Reutiliza el adaptador Ollama compatible con el perfil nativo y el generador
Qwen-Image pinned. Un proceso propio de texto por turno en puerto efímero, o un
proceso propio de imagen, nunca ambos residentes por decisión de Seiya. El slot
se mantiene hasta confirmar el cierre del árbol propio mediante OwnedChild.
Los nombres de fase indican el paso del adaptador, no una medición de pesos
residentes ni porcentaje de carga. El arranque del host no inicia inferencias.

No adopta el servicio compartido11434 ni cierra procesos ajenos. Los inventarios
vacíos son precondiciones, no prueba de unload; keep_alive=0 no se usa como
certificado de liberación. Cerrar procesos propios confirma su terminación, no
una cantidad física de RAM/VRAM libre o todas las cachés del sistema. Sigue
aplicando presión/swap/disco/límites/propiedad antes y durante cada operación.

Los pins, hashes y formatos nativos previos permanecen: Gemma, trabajador Qwen y
pipeline de imagen concretos. Encontrar cualquier Ollama instalado no acredita
compatibilidad con ese perfil. El catálogo de texto/imágenes por API HTTP sigue
permitiendo bindings sustituibles, pero **no adquiere unload gestionado genérico**.

## Activar imagen desde chat nativo

En el asistente, seleccionar el rol nativo de imagen y sus rutas compatibles,
revisar los efectos y guardar/iniciar. Una revisión nueva incluye chat_images=true
en desktop-manager.json; desktop.json anterior permanece como rollback. No hay
generación al configurar. Alternativa: perfil propio con chat_images=true,
images configurado y features.images=true; ver
[ejemplo no ejecutable tal cual](../contracts/desktop/v1/config.chat-images.example.json).
Un perfil viejo sin ese campo continúa con imágenes manuales hasta revisar.

El cerebro puede proponer image_generate solo para una petición explícita. Se
confirma primero el cierre de Ollama; después el broker ejecuta la imagen y
ofrece progreso, cancelar y PNG validado en el panel existente. No segundo turno
automático del cerebro. Usa el puente JS ya implementado, sin alterar diseño o
los seis assets congelados. Desactivar imágenes en Settings retira la tool; no
equivale a una confirmación instantánea de cancelación o descarga del proceso.

El límite completo3584bytes nativo no aumenta, también con schemas de imagen y
trabajador. Se admite antes de leer staging/inventario o arrancar el motor, incluso
para borradores. Si no cabe, context_exhausted, sin truncar historial ni fallback.
El control manual evita el turno del cerebro, pero conserva los límites de imagen.

## Estado, fallos y reinicio

/health.runtime_status.managed_residency muestra phase, role, admission,
owned_cleanup_confirmed y physical_memory_release_measured=false. Es estado del
host; no leer texto del modelo como progreso, permisos o evidencia de descarga.

Con el lease ya adquirido, antes de spawn se escribe y sincroniza
native-model.inflight en los dominios de coordinación estable/legacy existentes.
Solo el cierre nativo confirmado permite retirar el marcador verificando la
identidad del archivo y sincronizando el directorio. No se borra uno sustituido
por otro archivo. Crash/drop o spawn/cleanup inciertos retienen la intención y/o
cuarentena; otros hosts actuales rechazan el siguiente modelo, sin replay.

No hay recuperación automática. Si existe marcador pendiente: verificar procesos
propietarios, operaciones y recursos con intervención operativa; no eliminarlo
para reintentar. Los binarios antiguos no reconocen el journal nuevo: no usarlos
como rollback tras un crash sin revisar antes el runtime. Si no se puede persistir
cuarentena, se retiene el lease durante el proceso, sin garantía ante fallo de disco.

Pendientes: ensayo aislado y autorizado de compatibilidad, carga/descarga física,
imagen/calidad/cancelación reales y sesión sostenida. Sin instalación, modelos
descargados, gestores de ventanas, Docker, visión/control PC o cambios OpenCode.

Regresión offline:496 Rust+194 Python y5 suites Node correctas,24 ignored;
fmt/Clippy/diff-check y6 hashes congelados correctos. Las pruebas nativas emplean
modelos simulados y un helper propio sin pesos, no inferencia o VRAM real.
