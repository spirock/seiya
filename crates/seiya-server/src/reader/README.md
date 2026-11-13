# Lector experimental · componentes de S4-05

Esta librería **no está montada en el router normal ni habilitada por CLI**.
El constructor explícito `app_with_reader` monta el endpoint de ADR0030 solo
para integración de librería; requiere clave no vacía y no se usa en el servicio.
S4-06 añade extracción en el batch y S4-07 integra chat en perfil opt-in. El
endpoint devuelve texto o un estado de fallo, nunca un body HTML activo ni
éxito útil fingido por recibir HTTP200. No se ha probado contra páginas reales.
No anunciar `web_fetch` disponible. Prototipo Python y configuración intactos.

- `policy`: HTTPS443 sin userinfo, URL acotada, fragmentos eliminados; descarta
  IPs privadas/especiales y todo el resultado DNS si alguna IP es rechazada. IPv6
  admite un subconjunto conservador de2000::/3: puede rechazar destinos públicos.
- `transport`: resuelve una vez por salto y pasa una IP validada a TCP, conservando
  el hostname para TLS/SNI/Host. Rustls verifica certificados con raíces WebPKI.
  No proxy de entorno, cookies, auth, pooling, retries, sesiones de navegador,
  redirecciones implícitas, ALPN HTTP2 o fallback. DNS/conexión≤3s, respuesta≤10s,
  operación≤45s y el plazo menor proporcionado por el host prevalece.
- `wire`: HTTP/1 conservador: cabeceras≤64KiB, cuerpos acumulados de redirects
  y respuesta≤1MiB por página. Content-Length, chunked sin extensiones/trailers,
  o EOF. Rechaza duplicados, CL+TE, framing inválido y cuerpos truncados.
  Solicita identity y **rechaza gzip/br/deflate**: no hay descompresión ni soporte
  de compresión demostrado. Solo MIME HTML/texto y UTF-8 sin sustituciones.
- `Reader`: un slot. Timeout o cancelación de su future deja la instancia en
  cuarentena irreversible: no presume que el worker DNS bloqueante del SO haya
  acabado. No rearma por salud/memoria ni recupera automáticamente. Crear otra
  instancia no prueba cleanup; solo un operador podrá decidir recuperación.

`Network` es un adaptador confiable del host para probar sin red. Solo la
implementación `HttpsNetwork` fija IP y verifica TLS; los mocks no validan una
conexión real. Construir `HttpsNetwork::default` no abre sockets; invocar fetch
real sí hace red y requiere autorización. Los tests normales nunca lo invocan.

`fetch_report` conserva contadores de bytes entregados al parser también en
errores: no son tráfico TCP/cifrado, bytes de readahead TLS o del kernel. No
afirmar cota dura de tráfico on-wire o RAM del SO por aplicar límites de parsing.
Al exceder una cota, los contadores de diagnóstico pueden reflejar un byte extra;
no encajarlos en un frame con máximos fingiendo ceros. Antes de integrar frames,
definir un error externo sin éxito parcial y conservar las observaciones reales.

`app_with_reader` reutiliza guardas Host/Origin/auth, exige Content-Length y JSON
sin compresión, limita frame16KiB y lectura3s. Valida el frame antes del slot.
Un batch retiene el slot hasta3 fuentes únicas; el plazo compartido incluye la
lectura del frame y no se renueva entre páginas. Errores/límites no se convierten
en documentos; la salida se valida contra el contrato. Tests del router sin
listener prueban guardas, errores, correlación, deadline y cuarentena.

El usuario eligió cancelación explícita: `/internal/reader/v0.1/cancel`, mismo
secreto y guardas.202 reconoce solicitud, no cleanup ni reversión de un run ya
completado. Un read cancelado responde409 y queda en cuarentena si había I/O;
antes de DNS se libera el slot. Hasta128 tombstones sin expiración silenciosa
cubren cancelación anterior al upload;503 al agotar capacidad. Un cancel de otro
run no detiene el activo. El cliente de S4-07 debe enviar esa operación; perder
conexión no implica parada inmediata: queda el deadline como respaldo.
Los tests de caída de future **no prueban desconexión TCP**. TLS se prueba con
handshakes en streams de memoria y certificado sintético (nombre correcto,
nombre incorrecto y emisor no confiable), no contra servidores de Internet.
No activar en CLI/servicio por pasar estas pruebas offline.

Tests sin red: `cargo test -p seiya-server --lib reader:: --locked`.
Dependencias autorizadas: tokio-rustls0.26/ring y webpki-roots1, versiones exactas
resueltas en Cargo.lock. Licencias permisivas de los crates; no código copiado de
otro motor, cambios de modelo, instaladores, permisos del SO o servicios.

## Extracción y selección · S4-06

`extract` usa tokenizer HTML5 html5ever0.35, sin DOM o navegador. Selección propia
conservadora, no equivalente a árbol HTML de un browser: markup mal anidado puede
descartar contenido útil. Excluye zonas no útiles/hidden y prefiere main/article;
sin ellas filtra el cuerpo. Entidades se decodifican como texto, no se ejecutan.
No estilos computados, JS, fechas verificadas, paywalls o acceso a enlaces.
Entrada≤1MiB UTF-8, profundidad/atributos256, tokens100000, texto20000 por fuente
y30000 por batch. Heurística mínima80 caracteres: no mide calidad de resumen o
que una página sea un artículo; snippet/portada corta produce no_useful_text.

`selection` acepta únicamente IDs de texto correcto entregado en la respuesta
correlacionada del run. Rechaza IDs ajenos/duplicados, errores o texto de otro run.
8000 caracteres por fuente y16000 acumulados; recorte explícito y error cuando
no queda espacio para una fuente seleccionada, nunca descarte silencioso. No
sustituye el presupuesto de tokens efectivo de S4-07.

La salida es texto en JSON, no HTML sanitizado para innerHTML. S4-07 debe usar
textContent o un renderer con escape. Texto de inyección se conserva como dato
no confiable y no dispara herramientas ni se eleva a instrucciones del sistema.
