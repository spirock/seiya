# Limpieza del árbol de trabajo · 2026-10-19

Se retiraron del árbol activo el motor C, paquete Python, interfaces
web/escritorio, sitio, Docker, pruebas y documentación del proyecto anterior,
sus launchers y workflows. Se eliminaron **1.211 archivos** del árbol activo,
incluidos artefactos compilados locales identificados. No se usó `git reset`,
no se reescribió el historial ni se hizo commit o staging de los cambios.

Antes de retirar nada se guardó y verificó cada archivo por SHA-256 en:

```text
$HOME/Documents/seiya-backups/colibri-legacy-20261019-103000/
  legacy.tar.gz
  manifest.json
```

SHA-256 del archivo: `658aca5e0477d7fbcf517ff387b24527c831d3f021b2c116a16135a72797a135`.
El archivo ocupa una copia recuperable fuera del repositorio; no forma parte
del producto ni del empaquetado. Recuperar solo rutas seleccionadas en un
directorio separado: no extraer encima de Seiya y sobrescribir cambios nuevos.

Se conservaron:

- `.git` y `LICENSE`; la procedencia y avisos antiguos siguen en Git y backup.
- `swarm-app/`, benchmarks en `local-tools-tests/` y workflow de Seiya.
- Configuración externa de OpenCode/Seiya, runtime activo y modelos descargados.
- Historial de chats del navegador y sesiones de OpenCode.

Se mantienen las rutas actuales de la aplicación para no romper el servidor
en uso. `~/colibri-models/` es una ubicación histórica **externa** de pesos y
resultados que la configuración actual utiliza; no se borra ni se renombra.
El directorio físico del repositorio también conserva su nombre actual. Un
cambio de ruta requeriría detener servicios y mover sesiones explícitamente.
