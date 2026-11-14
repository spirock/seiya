# Publicación y distribución de Seiya

Repositorio público: https://github.com/spirock/seiya.
Operaciones Git: `git@github.com:spirock/seiya.git`, rama `main`.

## Separación del entorno de desarrollo

La publicación utiliza un snapshot independiente de fuentes, contratos, pruebas,
documentación de producto y workflows. No incluye el índice/historial del árbol
de trabajo anterior, pesos, sesiones, conversaciones, logs privados, credenciales,
artefactos generados o configuraciones personales. Se conservan licencia Apache2
del proyecto y atribuciones de dependencias; los modelos tienen licencias propias.

Instrucciones de agentes, archivos `.gitignore` y carpeta `plans/` son **locales**.
No se versionan en la rama publicada. Permanecen en disco para continuidad del
desarrollo y se excluyen de la copia de publicación. La hoja pública es
[ROADMAP.md](../ROADMAP.md), con estado técnico y próximos hitos, sin registro de
conversaciones o atribuciones personales. CI rechaza su reincorporación accidental.

La rama publicada se reinicializa desde un snapshot limpio, con el historial
anterior archivado y verificado solo localmente antes de sustituir la referencia.
Esta operación no borra el repositorio, su configuración o artefactos de Actions.
No es una acción automática del build ni se repite al actualizar la aplicación.
Eliminar la ascendencia Git de main no garantiza borrar objetos retenidos por
GitHub, referencias de ejecuciones antiguas, forks o cachés externas.

## Artefactos

- Rust macOS ARM64: .pkg/.dmg sin firma/notarización, app paralela al prototipo.
- Rust Linux x64: .deb GNU por arquitectura, requisito glibc del runner declarado.
- Rust Windows x64: ZIP portable y setup.exe per-user sin Authenticode.
- Binarios Rust también se entregan en archivos comprimidos por OS/arquitectura.
- Prototipo Python disponible mediante empaquetado explícito de rollback; no
  confundir sus paquetes anteriores con el payload Rust.

[Requisitos](PAQUETES_PRUEBA.md) · [Windows](WINDOWS_RUST.md) ·
[Operación macOS](APP_RUST.md) · [Paridad](PARIDAD_RUST.md).

Los paquetes no incluyen pesos, motores de inferencia o credenciales. Builds e
inspección no instalan, abren la app, inician modelos, cambian permisos o reinician
servicios. La selección de capacidades depende de la plataforma y del build.

## Enlaces de descarga en README

El snapshot actual mantiene README.md principal en inglés y readmees.md en
español con enlaces recíprocos. Downloads valida artefactos del mismo SHA/run
antes de escribir ambos bloques. Se requiere DMG macOS y PKG opcional separado;
el DMG contiene la app y el enlace Applications, no un instalador anidado.
El bot añade solo los dos documentos, sin planes/instrucciones/ignore privados.

El job `downloads` genera solo los bloques delimitados de README.md/readmees.md después de
contracts/build/windows correctos en `main`, nunca desde PR. Los IDs deben ser
no expirados y corresponder al mismo run/SHA, con los nombres de artefactos
esperados. Un build incompleto conserva los últimos enlaces verificados.

El job tiene permisos de escritura solo para la operación de documentación,
añade únicamente ambos README y comprueba que HEAD siga siendo el SHA compilado. No
usa force-push. El commit bot lleva `[skip ci]`; no modifica modelos, roles,
matriz de capacidades, gráficos o documentación fuera del bloque.

Son artefactos Actions, no releases permanentes. GitHub puede exigir sesión y
aplicar retención/caducidad. No se crean releases o actualizaciones instaladas
automáticamente. Firmas, avisos diarios de versión e instalación real son hitos
separados de la publicación de artefactos.

## Evidencia de referencia

### Incremento local · catálogo de texto y backend portable (sin publicar)

Regresión final:452 Rust+194 Python y4 suites Node correctas,24 ignored;
fmt/Clippy/diff-check correctos. Catálogo/roles sustituibles, HTTP local con
binding, locks/journal, rechazo de presupuesto sin despacho, cancelación y CAS
comprobados con fixtures. CLI offline comprueba cero conexiones. Frontend
HTML/CSS/JavaScript conservado permanentemente, seis assets SHA256 idénticos.
Tabla OpenCode distingue API parcial y ensayos históricos de integración real
del launcher; no plugin/configuración automática ni OpenCode modificado.

Paquete local macOS ARM64 reconstruido e inspeccionado sin instalar. Payload
expandido: allowlist, cabeceras/hashes de binarios release, --launch y catálogo,
contrato/operación incluidos y comprobados contra fuentes. DMG CRC/manifiesto
comprobados en montaje propio readonly/nobrowse/noautoopen, después desmontado.
BuildInfo `revision:null`: no inventar SHA/run para fuentes locales sin commit.

| Artefacto local final | SHA256 |
|---|---|
| Seiya-Rust-0.1.0-arm64.pkg | `df7e69ea5bdb928e6670cbb2b4046698e7cffe6bbc92d28a1db716df2f1570e1` |
| Seiya-Rust-0.1.0-arm64.dmg | `c75c1ceac74dc13224ae312a7310f038c8cb6eab97823e75b4344a3ad39b85aa` |

Avisos pkgbuild `write: Permission denied` y hdiutil deprecated conservados,
sin fallo de build/inspección. Targets Windows/Linux ausentes en este entorno:
no cross-build o descarga de toolchains; su payload/launcher se prueba con PE/ELF
sintéticos. No instalación, GUI, GPU/VRAM o motores reales del cambio acreditados.
No se han descargado/cargado pesos, iniciado/detenido motores o publicado cambios.
Los enlaces Actions siguen apuntando al build anterior.

### Incremento local web-first (aún no publicado)

`--launch` abre el shell web sin perfil/motores, con asistente y gestor de
asignaciones. Regresión offline:438 Rust+192 Python y4 suites Node correctas,
24 ignored; fmt/Clippy/diff-check correctos. Contratos y casos negativos de
auth/Host/Origin/cuerpos/CAS/roles/cancelación/migración comprobados con fixtures.
Windows comparte fuentes del shell y acceso --launch, pero no tiene backend de
inferencia. El target Windows no está instalado en el entorno local: su nuevo
binario no se ha compilado ni ejecutado; payload/ZIP/shortcut probados con PE
sintéticos. No atribuir al run anterior la validación de este código nuevo.

Build release macOS ARM64 y `.pkg/.dmg` locales reconstruidos sin instalar:
payload expandido, allowlist/Mach-O/hashes, scripts --launch y asistente embebido
comprobados. DMG CRC y manifiesto exacto comprobados mediante montaje propio
readonly/noautoopen, después desmontado. Sin app, navegador, modelos o runtime
ejecutados. `revision:null` identifica fuentes locales sin commit, no un SHA de
Actions. Avisos pkgbuild `write: Permission denied` y deprecación hdiutil sin fallo
del build; se conservan como limitación, no se ocultan.

| Artefacto local | SHA256 |
|---|---|
| Seiya-Rust-0.1.0-arm64.pkg | `4424ee47cb5db1f58f32d4b46d42205ed2898e17f635145c506bd407fa923172` |
| Seiya-Rust-0.1.0-arm64.dmg | `5dec4eabc6f3bd9f0dc9c65b086e446fd18cf4106fd21403f6b581080b18a49b` |

No se ha hecho commit/push, instalación o actualización de enlaces públicos en
este incremento. Finder/GUI/inferencia reales, firma/notarización y build nuevo
en Windows siguen pendientes. Asignar variantes no admitidas no las activa.

### Builds anteriores publicados

| Código / run | Resultado y alcance |
|---|---|
| `1d49370` / [43507170152](https://github.com/spirock/seiya/actions/runs/43507170152) | Snapshot raíz limpio: contratos, Windows, macOS y Linux correctos. Job downloads actualizó README automáticamente (commit bot `2b6fd49`); enlaces/figuras/tablas comprobados. No instalación o GUI real. |
| `b525f83` / [43487352926](https://github.com/spirock/seiya/actions/runs/43487352926) | Rust Linux/macOS: builds, extracción/allowlist/hashes y ayuda/versiones correctos; sin instalar. |
| `25909af` / [43493781845](https://github.com/spirock/seiya/actions/runs/43493781845) | Windows/Linux/macOS y contratos correctos; Node24 sin avisos Node20. Setup compilado con InnoSetup preinstalado, ZIP extraído, CLI/config y DLL auditadas; no instalación. |
| `15cc2f2` / [43502315485](https://github.com/spirock/seiya/actions/runs/43502315485) | Contratos, Windows y Linux correctos. macOS cancelado antes de ejecutar pasos (runner_id0, sin runner asignado); downloads omitido. No acredita fallo de código macOS ni nuevo conjunto completo de descargas. |

Launcher macOS/README: regresión local425 Rust+191 Python correctas,24 ignored,
3 suites Node, fmt/Clippy/release/diff-check correctos. Diálogos/browser/host
simulados; CSS conservado bajo autenticación loopback in-process. Modelos por
roles sustituibles y comprobación diaria de versiones siguen en diseño.

Paquete macOS reconstruido e inspeccionado sin instalar: payload expandido,
allowlist/formatos/hashes y scripts --launch, nota de paridad, CRC y manifiesto de
DMG comprobados en montaje readonly propio posteriormente desmontado. Persisten
avisos pkgbuild `write: Permission denied` y deprecación hdiutil sin fallo del build.
No GUI Finder, Gatekeeper, instalación/actualización/desinstalación en un equipo
destino ni inferencia desde la nueva entrada acreditadas por esas comprobaciones.
