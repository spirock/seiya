# Matriz de paridad · M3

No es declaración de sustitución de Python. Pruebas normales: fixtures/mocks;
los ensayos reales anteriores no acreditan automáticamente la nueva entrada GUI.
Tabla rápida por SO y arquitectura en README, sección Funciones por sistema
operativo. Sus símbolos separan binarios/CLI comprobados, funciones experimentales
y funciones ausentes; no convierten pruebas de fixtures en validación de dispositivo.

| Área | Rust actual | Evidencia/límite |
|---|---|---|
| Launcher macOS (fuentes nuevas) | Web primero sin perfil/motor; detección, revisión/confirmación y parada desde asistente | HTTP/DOM/filesystem sintéticos; GUI/inferencia real pendientes; build enlazado anterior usa diálogos |
| Gestor de modelos | Catálogo de texto: cerebro/trabajadores sustituibles y pool secuencial; ruta MLX/imágenes anterior separada | HTTP/bindings/recursos/locks/journal y CAS probados con fixtures; no catálogo universal o downloader |
| Diseño web | HTML/CSS existentes, JS de controles/historial añadido | CSS igual bajo cookie loopback, suites DOM; no rediseño |
| API `/v1/models`, chat/tools JSON | Implementada, worker/cola/límites | Fixtures compartidas y ensayos OpenCode anteriores; diferencias de schemas documentadas, no paridad absoluta |
| Streaming | SSE bufferizado | No tokens vivos; no prometerlo por abrir navegador |
| Chat nativo Gemma/macOS | Adaptador existente, mismo guard de memoria/lease | Smoke anterior aislado; nueva entrada GUI no ensayada con pesos |
| Qwen y trabajos | Borradores por turnos, opt-in | Fixtures/recovery; no ejecutar código/proyectos por activar switch |
| Imagen | Adaptador pinned macOS y API/UI opt-in | Imagen real anterior aislada, no campaña GUI conjunta o Windows/Linux |
| Configuración | Perfil v1 + settings CAS existentes | Guardar perfil no pisa elecciones CAS ni recupera cuarentenas |
| Historial chat | localStorage del navegador | Solo mismo origen/namespace, no sincronización entre dispositivos |
| Snapshots Rust de trabajos/PNG | Recuperación sin reejecución | Tests de reinicio/quota/corrupción; usar data_dir existente para recuperarlos |
| Buscar/navegar desde conversación | Sin paridad completa con Python | Reader Rust separado no es una tool activa del perfil Gemma por prosa |
| Consenso completo/herramientas SO | Sin paridad/pendiente según componente | No inventar capacidades; herramientas OpenCode no se ejecutan dos veces |
| Instalación, firma y actualización | Artefactos experimentales | CI inspecciona sin instalar; Gatekeeper/SmartScreen/dispositivo pendientes |
| Windows/Linux (fuentes nuevas) | Shell y catálogo de texto con conectores HTTP locales externos; roles persistidos tras confirmar | Código Rust y fixtures; motor externo ya preparado/dedicado, sin ejecutar binarios o imágenes; builds/GUI/inferencia reales por SO pendientes |

## Historial al abrir el launcher local

`http://127.0.0.1:8015` no comparte origen con HTTPS/IP privada de LAN. Los chats
anteriores siguen en almacenamiento del navegador del origen anterior. No se
copian, borran, resumen o reenvían automáticamente; tampoco se promete que otro
navegador conserve esa cookie. Se mantiene namespace Gemma entre modos solo/full
del mismo origen. No hay migración silenciosa de localStorage a snapshots Rust.

Configurar un perfil nuevo usa data_dir privado nuevo y no busca documentos o
historial externo. Para recuperar snapshots anteriores elegir explícitamente un
perfil existente con ese data_dir; la autoridad de coordinación sigue fija y los
marcadores de cuarentena no se limpian. Importar un perfil no ejecuta su contenido:
se valida y se muestran rutas antes de pedir inicio; solo se usa el snapshot aprobado.

## Puertas todavía pendientes

Arranque/stop mediante Finder, comportamiento real de los diálogos, sesión de chat,
imagen/Qwen, reinicio/RAM y prueba en otro equipo requieren ensayo real aislado
autorizado. Búsqueda, consenso y demás diferencias necesitan incrementos propios.
No retirar Python mientras queden estas diferencias o validaciones. Rollback
conserva app/comando Python separados y la CLI M2, sin borrar datos/modelos/locks.
