# Probar el instalador experimental

Los enlaces verificados están en el bloque Descargas del README del repositorio.
Cada conjunto identifica run y commit. Los nombres siguen usando versión0.1.0;
para distinguir builds comparar el commit en BUILD-INFO.json y SHA256SUMS, no
solo el nombre del archivo. Los artefactos Actions pueden exigir sesión GitHub.

## macOS Apple Silicon

1. Descargar el artefacto macOS ARM64 del run indicado y extraer el ZIP de Actions.
   En el nuevo formato, el DMG contiene Seiya Rust.app y un acceso a Aplicaciones;
   arrastrar la app a Aplicaciones y expulsar el volumen. No se arrastra el DMG.
   Los builds anteriores contienen un PKG dentro del DMG; revisar el commit.
2. Cerrar una instancia anterior de Seiya con su control Detener/Cerrar y esperar
   confirmación. No cerrar Ollama compartido ni borrar datos, locks o journals.
3. Abrir **Seiya Rust.app** desde Aplicaciones. El PKG se ofrece como descarga
   separada opcional: instala la misma app y wrappers de terminal seiya-rust y
   seiya-rust-server. No hace falta usar ambos formatos.
   El paquete no incluye modelos ni motores y no los descarga automáticamente.
4. Debe abrirse la web local de configuración sin inferencia automática. Revisar
   la ruta Ollama y su aviso encontrado/no encontrado/ruta guardada inválida.
   Encontrar el archivo no prueba compatibilidad con el perfil nativo MLX pinned.
5. Configurar rutas/modelos compatibles y revisar los efectos antes de iniciar.
   El catálogo HTTP externo y el perfil nativo son entradas distintas; no asumir
   que una instalación estándar de Ollama ofrece el generador de imágenes.
6. Si se activa chat_images nativo, probar primero un mensaje breve. La generación
   de imágenes se registra por petición explícita y termina en el panel de trabajos;
   un mensaje del chat no acredita por sí solo un PNG completado.

Los paquetes no están firmados/notarizados. Si macOS bloquea la instalación,
anotar el aviso exacto para feedback y decidir según la política del equipo.
**No desactivar Gatekeeper ni la protección global del sistema.** Esta entrega
no acredita instalación, GPU, RAM/VRAM libre, compatibilidad de todos los motores
o sesiones prolongadas por haber superado compilación y pruebas sintéticas.

### Si «Revisar configuración» parece no responder

La revisión no carga modelos. Comprueba si falta el campo obligatorio Cerebro o
alguna ruta; el resultado debe verse junto al botón y la aprobación aparecer debajo.
En builds anteriores el mensaje estaba al inicio del panel desplazable: subir
hasta arriba permite leer el error que quedó fuera de la vista.

Si se ha seleccionado imagen pero sd-cli/assets están vacíos, completar esas rutas
o pulsar «Desactivar imágenes en este borrador» para preparar solo texto. En builds
anteriores, vaciar manualmente Pipeline de imágenes. Esa acción no guarda ni inicia
nada: aún hay que revisar y confirmar un motor/almacén de texto válido. Nunca
interpretar «Guardar asignaciones en este navegador» como activar los modelos.

### OpenCode muestra «API key required»

Ese401 indica que el servicio API no recibió autorización Bearer; no demuestra un
fallo del modelo. La sesión/cookie web y la API compatible con OpenCode son accesos
distintos; la URL /trial/ del navegador no es el endpoint OpenAI /v1.
El proveedor seiya debe usar la misma clave privada configurada para el servidor
API al que apunta baseURL. No sirve inventar una clave, reutilizar la cookie web
ni desactivar autenticación para ocultar el error. El ejemplo legacy con apiKey
local no acredita acceso a un servidor autenticado con otra clave.
Comprobar únicamente baseURL y presencia/origen de apiKey, sin publicar su valor.
Las variables {env:...} deben estar disponibles para el proceso que carga la
configuración; una variable ausente se convierte en cadena vacía. La referencia
oficial V2 permite también {file:...} para claves privadas fuera del JSON.
No reiniciar el servicio compartido de OpenCode ni arrancar otro coordinador sobre
el runtime activo para diagnosticarlo. Ver https://opencode.ai/docs/config.

## Windows y Linux

Elegir x64 y el formato correspondiente (.exe/ZIP en Windows, .deb en Linux).
La interfaz común y el catálogo HTTP no incluyen motores/pesos. Inferencia,
imagen nativa macOS y MLX/Metal no se vuelven portables por instalar el paquete.
Consultar WINDOWS_RUST/BACKENDS_LOCALES y la glibc mínima del .deb. Las pruebas
de compilación/payload no acreditan GUI o inferencia reales en esos equipos.

## Feedback útil y privado

- Run/commit del build, SO/versión, arquitectura y RAM del equipo.
- Si descarga, instalación y apertura de la web funcionaron.
- Paso exacto del fallo y texto de error; captura solo del área relevante.
- Qué motor/modelo se configuró y si el estado quedó bloqueado, en ejecución o
  confirmó cierre. Indicar si había otra instancia activa, sin matar procesos.
- Si el problema aparece al volver a abrir y si se conservó el historial.

No compartir tokens, API keys, cookies, URLs completas de bootstrap/sesión,
conversaciones, rutas personales o logs enteros sin revisión. Una captura del
estado o un error redactado suele bastar. No borrar native-model.inflight,
http-*.inflight ni cuarentenas para reintentar; requieren revisión operativa.
El prototipo anterior y los datos se conservan, pero tras cierre incierto no
usar un binario viejo como rollback hasta verificar procesos/operaciones pendientes.
