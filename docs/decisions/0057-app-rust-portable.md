# ADR0057 · contrato de entorno para la aplicación Rust

Estado: aceptado para primer incremento offline; no promoción del host.

Actualización M2: ADR0058 integra el contrato en una entrada de host explícita;
las referencias a «M2 deberá» de esta decisión describen las puertas originales,
no una afirmación de validación real. La comprobación offline conserva su alcance.

## Objetivo y alternativas

Pasar la aplicación/coordinación a Rust conservando UI y adaptadores nativos.
Reescritura completa de UI/motores descartada: amplía alcance y no demuestra
ahorro de pesos/RAM. Sustituir directamente Python por los hosts de ensayo
descartado: dependen de rutas privadas y controles de exclusión compartidos.
Elegimos contrato JSON v1 explícito y comprobador offline antes de integrar rutas.

## Contrato M1

`seiya-desktop --check-config ARCHIVO` solo lee un archivo regular no symlink,
máximo64KiB. Sin defaults HOME/temp/PATH, sustitución de variables, descargas,
servicios o ejecución de binarios. Target inicial `macos`, versión1. Paths absolutos
canónicos existentes: data_dir y coordination_dir privados; model_store,
llm_binary e images opcional {binary, assets_dir}. Features false por defecto;
images=true exige definición images. Cambiar JSON no activa modelos ni concede
permisos. Nombres/tags/digests del ensayo no se hacen intercambiables por ello.

Datos y coordinación no pueden solaparse entre sí ni con modelos/binarios/assets.
Directorios privados sin permisos group/other; ejecutables sin escritura group/
other. No se exige chmod de pesos del usuario ni se modifica ningún archivo.
JSON no contiene claves/TLS; su integración y bootstrap son una fase posterior.
Éxito significa solo estructura/metadata de filesystem comprobadas en ese instante.
No inspecciona pesos, no ejecuta --version y no certifica licencia, runtime,
ownership de procesos, RAM, plataforma efectiva ni inferencia. Paths no se
imprimen al reportar errores. No es una frontera contra TOCTOU; M2 deberá revalidar
al abrir/iniciar y conservar los controles existentes.

## Migración y consecuencias

API nueva aislada; CLI de hosts antiguos y API OpenAI/nativa sin cambios. No
consumir aún este contrato en un host con otro lease: cambiar coordination_dir
no debe eludir la cuarentena compartida. M2 requiere diseñar compatibilidad del
dominio de exclusión y rechazo de dos coordinadores antes de habilitar arranque.
Rollback M1: dejar de usar el comprobador; no ha escrito ni migrado datos. No
retirar Python hasta paridad y pruebas reales. Un motor externo puede seguir
usando Python; eliminar esa dependencia no es condición de M1.
