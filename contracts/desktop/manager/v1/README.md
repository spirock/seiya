# Gestor desktop v1 · experimental

`desktop-manager.json` es un documento privado del asistente macOS, distinto de
desktop/v1, del wire OpenAI y de los settings CAS. Campos superiores estrictos:
`version:1`, `draft` y `profile`. [Ejemplo no ejecutable](config.example.json).

`draft` contiene llm_binary, model_store, image_binary, image_assets y roles.
Roles: brain (ID no vacío), workers (lista de hasta16 IDs únicos, sin vacíos),
image (ID, vacío desactiva imágenes). Rutas≤4096bytes, IDs≤200bytes, sin NUL;
IDs sin saltos de línea. Documento≤65536bytes, archivo regular privado, sin
symlinks. `profile` conserva el perfil desktop/v1 completo y las rutas operativas
del historial/coordinación, no solo las dependencias elegidas en el formulario.

Los IDs son **asignaciones**, no una oferta universal de modelos ejecutables.
Admisión actual: brain=gemma4:e2b-mlx; workers=[] o [qwen3.5:9b-mlx]; image vacío
o qwen-image-2.1-turbo-q4. Otros valores pueden persistirse, pero no arrancarse.
Una revisión nueva con rol imagen seleccionado incluye chat_images=true en el
perfil y avisa en efectos; no cambia desktop.json ni activa una generación al
guardar/iniciar. Perfiles anteriores sin ese campo mantienen imagen manual hasta
revisión. El campo pertenece al perfil nativo, no a diagnostics.features.
No descargas ni cambios silenciosos de modelo/proveedor. Los pesos, runtimes,
pins, licencias y recursos todavía deben satisfacer el adaptador existente.

El asistente importa rutas del desktop.json anterior cuando no hay documento
nuevo; no sobrescribe aquel archivo. Guardado atómico600 con lock/comparación
de revisión SHA256 (ambos archivos); el navegador recibe solo esa revisión.
La revisión de filesystem no acredita inferencia, ACLs, ownership o ausencia de
carreras de sustitución externas. Mantiene los límites del contrato desktop/v1.

## Endpoints internos del shell

Bajo prefijo autenticado aleatorio, no API LAN/pública ni contrato de un modelo:

| Método/ruta relativa | Efecto |
|---|---|
| GET api/status | Plataforma/arquitectura, adaptador disponible, fase y error |
| GET api/detect | Rutas conocidas/perfil y revisión; sin ejecutar motores |
| POST api/pick {field} | Selector macOS constante para uno de cuatro campos |
| POST api/review {draft,baseline} | Validación; prepara directorios propios; aprobación de un uso |
| POST api/confirm {approval,start} | Consume bytes revisados y guarda; inicia solo si start=true y roles admitidos |
| POST api/enter | Entrega una vez el bootstrap del chat; después su home con cookie |
| POST api/stop | Solicita parada; no declara cleanup completado por responder200 |
| POST api/quit | Revoca admisión y cierra tras esperar cleanup |

Host/Origin exactos, cookie HttpOnly/SameSite, sin CORS ni secretos en HTML o
localStorage; cuerpos128KiB/5s, cuatro lecturas simultáneas y una mutación activa.
Una aprobación no se reintenta ni se recupera automáticamente tras recarga/error.
Bloqueo por cleanup incierto impide otro inicio, sin borrar cuarentenas.

Windows/Linux no escriben este documento ni ejecutan el perfil macOS. Comparten
la web y pueden guardar/recuperar **solo IDs** como borrador explícito en
localStorage `seiya.desktop.roles.v1`, no activar modelos. No se sincroniza.
