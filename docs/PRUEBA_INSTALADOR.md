# Probar el instalador experimental

Los enlaces verificados están en el bloque Descargas del README del repositorio.
Cada conjunto identifica run y commit. Los nombres siguen usando versión0.1.0;
para distinguir builds comparar el commit en BUILD-INFO.json y SHA256SUMS, no
solo el nombre del archivo. Los artefactos Actions pueden exigir sesión GitHub.

## macOS Apple Silicon

1. Descargar el artefacto macOS ARM64 del run indicado y extraer el ZIP de Actions.
   El DMG contiene el mismo PKG, nota y hashes; no hace falta instalar ambos.
2. Cerrar una instancia anterior de Seiya con su control Detener/Cerrar y esperar
   confirmación. No cerrar Ollama compartido ni borrar datos, locks o journals.
3. Instalar manualmente el PKG y abrir **Seiya Rust.app** desde Aplicaciones.
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
