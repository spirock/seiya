# Paquetes y ejecutables de prueba · Rust experimental

Sin pesos, credenciales, sesiones o motores MLX/Ollama/sd.cpp incluidos. No
instalación, carga de modelos, servicios o permisos automáticos. Artefactos de
prueba sin firma/notarización; no release autónoma ni paridad completa con Python.

## Instaladores Rust nuevos

**Windows x64:** añadido pipeline nativo para ZIP con `.exe` y setup.exe per-user.
Artefactos `seiya-rust-Windows-X64-portable` y `seiya-rust-Windows-X64-installer`;
disponibilidad solo tras run correcto. CLI/API discovery, **no web/modelos Windows
validados**. [Instrucciones y límites Windows](https://github.com/spirock/seiya/blob/main/docs/WINDOWS_RUST.md).
Run43493781845 SUCCESS confirmó ZIP portable y setup.exe, además de Linux/macOS,
con acciones y suites en Node24. No instalación real o inferencia acreditadas.

Workflow actualizado: artefactos `seiya-rust-packages-OS-ARCH`. Su disponibilidad
se acredita con un run concreto, no por modificar el YAML. Los enlaces del run
anterior43456809371 siguen conteniendo los **paquetes Python antiguos**.
Run43487352926 confirmó paquetes Rust Linux/macOS para b525f83; no incluía Windows.

- Debian: `seiya-rust-experimental_0.1.0_amd64.deb` o `_arm64.deb`. Instala
  `seiya-rust` (ayuda/chequeo offline) y `seiya-rust-server` (API Rust existente).
  No depende de Python; requiere GNU libc/libgcc compatibles y runtime externo
  para inferencia. Linux **no soporta** el host --serve macOS, MLX/Metal o imagen
  nativa por instalar este paquete. Entrada de escritorio muestra ayuda CLI.
- macOS: `Seiya-Rust-0.1.0-ARCH.pkg` y `.dmg` con el mismo .pkg/nota/hashes.
  Instala `Seiya Rust.app`, `seiya-rust` y `seiya-rust-server`. macOS13+,
  arquitectura concreta (amd64=Intel, arm64=Apple Silicon), no universal.
  El DMG es un contenedor, no un motor ni un ejecutable independiente.
- `.app` nueva abre launcher nativo **Configurar/Abrir perfil**, revisión y
  confirmación antes de iniciar la web solo en este Mac. Sin comandos/TLS para
  loopback; LAN/TLS conserva entrada manual. No modelos, descargas o prompts
  automáticos. El diseño web está embebido en Rust; GUI real/paridad completa
  todavía pendientes. Builds antiguos mostraban ayuda en Terminal. APP_RUST.md
  detalla perfiles, requisitos y cierre.

Payload allowlist: seiya-desktop y seiya-server, wrappers, configuración de
ejemplo, notas, licencia/avisos y BUILD-INFO.json con hashes/target/revisión si se
proporcionó. Sin servidor .py, instaladores anidados, pesos o llaves privadas.
Python sigue siendo herramienta de **build**, no dependencia del payload Rust.

Los binarios `seiya-rust-OS-ARCH.tar.gz` se entregan aparte en Actions. Incluyen
ahora seiya-desktop además de seiya-server y hosts de ensayo macOS. Los hosts de
ensayo conservan defaults antiguos; preferir entrada configurada seiya-desktop.
No iniciar coordinadores diferentes sobre el mismo runtime fuera del contrato.

## Construir sin instalar

Primero compilar desde las fuentes correspondientes (sin arrancar modelos):

```sh
cargo build --release --locked -p seiya-server --bin seiya-desktop --bin seiya-server
# macOS arm64; Intel usa --arch amd64:
python3 swarm-app/packaging/build.py dmg --bin-dir target/release --arch arm64 --output /ruta/salida
# Linux GNU, con binarios ELF de esa arquitectura:
python3 swarm-app/packaging/build.py deb --bin-dir target/release --arch amd64 --libc-min 2.39 --output /ruta/salida
```

`--libc-min` del ejemplo **no se adivina**: reemplazar por versión del runner GNU
real de compilación (`getconf GNU_LIBC_VERSION`), conservadora para ese build.
CI usa su versión real; un runner reciente puede producir un .deb no instalable
en distribuciones más antiguas. No se acredita compatibilidad con musl o cualquier
Linux por tener ELF. Readelf/dependencias e instalación en equipo destino pendientes
de la evidencia concreta. Para ampliar compatibilidad hace falta un baseline de
compilación más antiguo validado, no rebajar Depends a mano.

`make mac`/`make dmg` usan RUST_BIN_DIR y RUST_ARCH; `make deb` además LIBC_MIN.
El empaquetador no compila, ejecuta binarios, instala ni descarga dependencias.
Rechaza binarios de otra plataforma/arquitectura, scripts y symlinks. Cabecera
válida no demuestra compilación fiable: usar binarios del snapshot comprobado.
`--revision SHA_COMPLETO` registra procedencia explícita, no inventa la del árbol
Colibri. Sin esa opción, revisión=null. SHA256SUMS solo lista outputs de esa llamada.

## Inspección y límites

Debian: `dpkg-deb --info ARCHIVO.deb`, `--contents` y `--extract` a staging privado.
macOS: `pkgutil --payload-files ARCHIVO.pkg`, `pkgutil --expand-full ARCHIVO.pkg
DESTINO_NUEVO` y `hdiutil verify ARCHIVO.dmg`. Sin ejecutar installer.
Después: `python3 swarm-app/packaging/inspect_rust.py PAYLOAD --platform macos
--arch arm64` (Debian: --platform linux y arquitectura correspondiente).
Comprueba allowlist/formato/arquitectura/hashes, sin ejecutar el contenido.
CI ejecuta solo --help/--version del desktop extraído, no servidores o inferencia.

Ayuda/versiones correctas y checksum válido no acreditan instalación, Gatekeeper,
firma/notarización, API/modelos, sesión larga, presupuesto RAM o navegador gráfico.
No modificar confianza global para saltar alertas de paquetes sin firmar. Instalación
y permisos los decide el propietario; no se realizan como parte de esta entrega.

## Convivencia y rollback

Nombres/identificadores nuevos no sobrescriben `Seiya.app`, comando `seiya` o
paquete `seiya` Python. No migran/borran datos. No desinstalar Python al probarlos.
Rollback: seguir usando explícitamente el launcher anterior. No borrar modelos,
historial, locks o cuarentenas al retirar los binarios experimentales.

El prototipo anterior se puede construir explícitamente con `--implementation
python` (o make deb-python/mac-python/dmg-python). Sus paquetes antiguos sí
necesitan Python3.10+ y no incorporan las opciones Rust por compartir repositorio.
Firma, SBOM formal e instalación/actualización reales quedan fuera de lo validado.
