# ADR0039 · ensayo secuencial con caché recuperable

2026-11-11. Usuario pide avanzar tras explicar que la RAM de archivos en caché
puede recuperarse automáticamente. Autoriza reanudar las dos pruebas individuales
con vigilancia, no concurrencia, vaciado de cachés o modificación de servicios.

## Decisión acotada

La guarda global existente de≥2GiB de páginas libres **no cambia**. Solo este
ensayo de caracterización usa una excepción explícita: presión normal antes y
durante, sin modelos compartidos residentes, lease exclusiva,≤120s por modelo,
salida≤256tokens/contexto candidato4096/thinking=false, una petición por tag.
No admite jobs de producción o ambas cargas simultáneas. Ante fallo se aborta
campaña sin retry. La muestra anterior de memoria no es un gate fresco.

Registrar free/inactive/file-backed/purgeable y swap por separado. Caché no es
una reserva garantizada; las categorías se solapan, no se suman como memoria
disponible. Tampoco memory_pressure -Q expresa GiB garantizados para el modelo.
No inventar un umbral de disponibilidad con esa suma. La evaluación de capacidad
en este ensayo es experimental: no garantiza que quepa o evita jetsam/OOM.

Supervisor externo muestrea presión y swap, para ante warning/critical, dato
faltante o aumento de swap>512MiB desde preflight. Lecturas con timeout; deadline
incluye carga y respuesta. Muestreo best-effort≤1s, no garantía hard-real-time.
Cancelar HTTP no prueba fin: cerrar solo árbol propio y comprobar salida/puerto
antes de liberar lease o siguiente tag. Error de limpieza deja campaña abortada.

Python se limita al adaptador operativo macOS (Popen, sandbox-exec, libproc,
HTTP loopback, lease y señales). Política pura y validación de respuesta en Rust,
con fixtures sintéticas. No dependencias nuevas. No integración del launcher en
Seiya ni cambio de APIs públicas. No delegación autorizada.

El almacén privado contiene solo manifiestos/blobs seleccionados por digest,
enlaces hacia pesos existentes. Sandbox heredado niega file-write* en raíz
original; comprobar denegación sobre fixture sintética, directa y por enlace,
antes de conectar pesos. NOPRUNE solo defensa adicional, no protección suficiente.
No escribir en pesos para probar la protección. HOME privado, NO_CLOUD, sin
proxies; no afirmar sandbox de red ni aislamiento contra código malicioso.

Alternativas: seguir exigiendo páginas totalmente libres ignora caché recuperable;
purge o cerrar servicios del SO altera host sin necesidad; una suma de métricas
solapadas produce falsa seguridad. Se elige calibración vigilada y reversible,
no autoadmisión. Riesgo residual de presión entre muestras declarado.

Rollback: no ejecutar el harness y mantener gate global original; cerrar solo
procesos propios, conservar pesos/configuraciones/historial/preview. Pasar las
dos pruebas no habilita concurrencia o estabilidad sostenida. Registrar resultados
y métricas ausentes, sin trasladar medidas E4B a E2B.
