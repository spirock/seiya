# Iconos de Seiya

Imagen aceptada provisionalmente: `docs/assets/seiya-icon-proposal.png`.
Generada a petición del usuario a partir del concepto del banner: degradado azul
oscuro y Pegaso como constelación cian/violeta. No modifica los seis assets web
congelados ni incluye fuentes/pesos de modelos. No se afirma exclusividad jurídica.

Recursos en `swarm-app/packaging/icons/`:

- `seiya.icns`: macOS, tamaños16/32/64/128/256/512/1024.
- `seiya.ico`: Windows, tamaños16/24/32/48/64/128/256, PNG-compressed.
- `seiya-<tamaño>.png`: Linux hicolor,16/24/32/48/64/128/256/512/1024.
- `manifest.json`: SHA256 de la fuente aceptada y cada derivado.

## Uso en paquetes

macOS instala el ICNS en Resources y registra `CFBundleIconFile` en Info.plist,
tanto en el PKG como en la app de arrastrar del DMG. Linux instala PNG en
`/usr/share/icons/hicolor/<tamaño>x<tamaño>/apps/seiya.png`, con `Icon=seiya` en
el archivo `.desktop`. No añade scripts de instalación o refrescos de caché.

Windows incluye `seiya.ico` en el ZIP/payload. InnoSetup lo utiliza para el setup,
accesos de inicio/ayuda y entrada de desinstalación. El PE Rust portable y el
archivo `.cmd` no reciben un recurso incrustado; su icono propio en Explorer
puede seguir siendo genérico. No se añade una toolchain Windows para modificarlo.

BUILD-INFO y los inspectores verifican hashes y allowlists de iconos, además de
los binarios. Usar herramientas de inspección de la misma revisión del paquete.
Generar recursos no actualiza instalaciones existentes ni publica instaladores.
La apariencia real/cachés de Dock, Finder, Explorer o un escritorio Linux requiere
prueba manual en esos sistemas, no se acredita con tests de archivos.

Comprobación local: ICNS/ICO decodificados con sips; PKG/DMG macOS arm64
reconstruidos e inspeccionados sin instalar. ZIP Windows y DEB Linux recompuestos
con binarios610d697 previamente verificados, únicamente para validar el nuevo
payload de iconos (no nuevo build de código). InnoSetup/atajos probados con mocks,
sin compilar setup.exe real. Regresión offline503 Rust+199 Python,10 suites Node
correctas,25 ignored; fmt/Clippy y6 hashes frontend congelados correctos.

## Regenerar sin instalar dependencias

En macOS con `/usr/bin/sips` preinstalado:

```sh
python3 swarm-app/packaging/generate_icons.py
python3 -m unittest discover -s swarm-app/tests -p 'test_packaging*.py'
```

El script redimensiona únicamente la imagen local aceptada y escribe contenedores
ICNS/ICO con Python estándar. Revisar visualmente los derivados y el manifiesto:
versiones distintas de sips pueden producir bytes distintos. CI empaqueta recursos
ya generados; no convierte, descarga o llama a modelos/red. No necesita Pillow,
ImageMagick, iconutil o sips en Linux/Windows. El PNG fuente no se mete en payloads.

Rollback: revertir referencias de iconos, allowlists y archivos de este incremento,
sin tocar perfiles, historial, modelos o cuarentena del motor.
