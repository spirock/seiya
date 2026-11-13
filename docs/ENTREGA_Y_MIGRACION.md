# Entrega, migración y aceptación

## Reglas

1. Mantener el prototipo operativo mientras se sustituye una capa cada vez.
2. No confundir «contrato», «compila», «unit tests» y «validado en dispositivo».
3. Cada release registra versiones exactas, configuración, hardware, fixtures,
   resultados, latencia, memoria, incidencias y límites no verificados.
4. No fijar promesas de tokens/s, batería, 32K o modelos móviles sin mediciones.
5. No migrar directorios externos de pesos ni historial por cambiar de branding.

## Etapas y puertas de aceptación

### 0 · Base Seiya y contrato (esta entrega)

- Retirar legado del árbol activo con backup verificado y mantener Git.
- Raíz dedicada a Seiya: documentación, contratos Rust/JSON, tests y empaquetado.
- Pruebas del prototipo y del crate Rust; ejemplos consistentes.
- El servidor activo sigue siendo Python. No declarar app Rust/iOS terminada.

### 1 · Núcleo Rust equivalente al prototipo

Incremento acotado completado: Sprint 01, un worker opt-in y herramientas externas
con dos smoke tests OpenCode/Gemma. **No completa esta etapa de paridad**: faltan
consenso, UI/persistencia, cancelación real y otras puertas de aceptación de abajo.
Operación, diferencias y rollback: `OPERACION_RUST_EXPERIMENTAL.md`.

- Implementar servidor/broker/scheduler, configuración y adaptador de runtime.
- Paridad `/v1/models`, chat, structured tools, auth, Host/Origin, límites y errores.
- Mismas fixtures para Python y Rust; cero doble ejecución de herramientas OpenCode.
- Cola con límites y cancelación propagada; no cargar dos modelos por accidente.
- Propuestas/votos inválidos se rechazan y no se presenta mayoría inexistente.
- Persistencia y eventos con replay. Fuzzing de parsing y límites.
- Golden tests de UI con envío, recuperación, errores, borradores e historial.
- Comparar tiempo/memoria del coordinador sin atribuir mejora del runtime a Rust.
- Activación con flag y rollback; no sustituir la instalación sin pruebas de paridad.

### 2 · Herramientas de escritorio y visión

La etapa de paridad Rust incorpora después el gestor de memoria especificado en
`MEMORIA_ADAPTATIVA.md`: baseline, presupuestos, leases, invalidación de cachés y
descarga por inactividad, primero en macOS. Las mejoras se validan antes de
habilitarlas por defecto; el store MoE/SSD queda como experimento posterior,
no como dependencia de visión, Mail o la primera app iOS.

- `apps_open` en allowlist, y Mail **solo lectura** como primer adaptador.
- Datos de prueba sintéticos/cuenta de test, permiso concedido y denegado,
  múltiples cuentas, buzones vacíos, mensaje largo y fallo de sincronización.
- Verificar que listar/leer no envía, elimina ni marca mensajes sin autorización.
- Imagen sintética: OCR de texto y pregunta visual; confirmar que no se responde
  solo por nombre de archivo. Límite de resolución y medición de memoria.
- App launch, captura y UI actions con targets/confirmaciones y redacción.
- Prompt injection en correo, página e imagen no provoca lectura/envío externo.
- Rutas, symlinks, DNS rebinding, redirects y datos comprimidos hostiles.
- No probar envío real, borrado o interacción con cuentas personales de forma automática.

### 3 · UI compartida y móvil

- Elegir contenedor/bridge por prueba técnica, no por promesa de portabilidad.
- Primera app en iPhone 15 Pro Max por Xcode; signing del usuario y Developer Mode.
- Modelo cuantizado pequeño; comparar Phi/Gemma/alternativas soportadas sin
  asumir que un checkpoint desktop cabe o que su parser funciona en iOS.
- Sin conexión al Mac: conversación y herramienta local sobre documento elegido.
- Sin Internet: no afirmar búsqueda exitosa. Con Internet: fuentes y fallo real.
- Pruebas de suspensión, reapertura, modo avión, falta de espacio, presión de
  memoria, thermal throttling y sesiones sostenidas con batería monitorizada.
- Límite de contexto móvil medido, no copiado del desktop. Una sola inferencia.
- Rutas iCloud/document providers y revocación de acceso; no lectura global.
- Tools no disponibles no se anuncian. Mail desktop/shell no se presentan como locales.
- TestFlight y App Store son pasos posteriores con revisión legal/técnica propia.

### 4 · Enjambre completo y modo remoto opcional

- Separar propuestas, aplicación de patches y ejecución de pruebas aisladas.
- Ejecutar fallos deliberados de tests y revisiones: ningún voto puede maquillarlos.
- Medir mejora de calidad frente a worker único con el mismo presupuesto.
- Emparejamiento seguro iPhone ↔ host, TLS, scopes y revocación.
- Destino visible antes de enviar; cancelación/desconexión sin duplicar acciones.
- Rechazar remote en `local_only`. No publicar el runtime de inferencia en la red.

## Matriz mínima de tests

| Capa | Positivos | Negativos |
|---|---|---|
| Contratos | Roundtrip, ejemplos y versión | Extras, IDs vacíos, payload grande |
| Políticas | Tool anunciada, grant y confirmación | Tool desconocida, OS incompatible, falta de grant |
| Contexto | Varias tools y resumen útil | Bucle de compactación, fijo que no cabe |
| Cola | Dos clientes, orden/avance medido | Cola llena, timeout, cancelación y fuga de slots |
| Sesiones | Refresh, replay, borradores | Corte durante escritura, cuota llena, corrupción |
| API OpenCode | Read/edit/shell reales aislados | Tool inválida, respuesta truncada, auth y schema |
| Mail | Listar/leer/ resumir test inbox | Denegación, inyección, cuenta incorrecta, efectos ocultos |
| Red | Search/fetch con fuentes | SSRF, rebinding, redirects, compresión, timeout |
| Multimodal | Imagen y OCR reales | MIME falso, imagen enorme, runtime solo texto |
| Móvil | Local offline y documentos elegidos | Suspensión, jetsam, calor y permisos revocados |

## Instaladores y soporte

- Artefactos por target/arquitectura, hashes, SBOM y licencias de redistribución.
- Actualmente `.pkg` no firmado/notarizado y `.deb` sin instalación Linux local
  verificada. No elevar ese estado por añadir el contrato o compilar un crate.
- No incluir cuentas, rutas privadas o credenciales en binarios/configuración.
- Instalación/desinstalación limpia, actualización sin perder sesiones y rollback.
- Antes de publicar: evaluar las dependencias de UI/runtimes y los requisitos
  concretos de permisos/entitlements/distribución de cada plataforma.

## Decisiones abiertas que requieren prototipo

- Contenedor UI definitivo y bridge Rust ↔ Swift.
- Motor/modelo/cuántización para cada dispositivo y calidad en español.
- Entitlements/canal de distribución del adaptador Mail/control de pantalla.
- Esquema transaccional final, política de cifrado y backups de conversaciones.
- Límites de memoria, contexto, herramientas por run y tiempos por dispositivo.
- Versionado estable del protocolo tras sus pruebas de interoperabilidad.

No se incluye una fecha ni un coste de entrega garantizados. Una fase se
considera hecha cuando aporta implementación y evidencia, no solo esta lista.
