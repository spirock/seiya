# ADR0076 · Diseño propuesto de actualizaciones y releases

**Estado: propuesta, no implementada.** Seiya debe seguir funcionando sin
Internet. Consultar actualizaciones no equivale a instalar ni ejecutar nada.

## Alternativas

1. **Instalador automático:** cómodo, pero exige resolver firma, confianza,
   permisos, reversión y sustitución de procesos en cada plataforma. Rechazado
   para esta etapa: paquetes actuales no están firmados.
2. **Consultar un manifiesto y abrir la release bajo acción del usuario:**
   permite informar de una versión posterior manteniendo instalación manual.
   Seleccionado como dirección de diseño, no como contrato ya desplegado.
3. **Sin consultas:** conservar uso totalmente offline. Siempre disponible;
   una consulta fallida no degrada chat ni fuerza cambios de configuración.

## Política de confianza y privacidad

- Red solo con consentimiento explícito; sin procesos en segundo plano cuando
  Seiya está cerrada. Si se autoriza, como máximo una comprobación al abrir y
  posteriormente una por 24 horas mientras la aplicación está abierta.
- HTTPS a una dirección fija controlada por el proyecto, sin credenciales,
  conversaciones, identificadores persistentes ni telemetría de uso. Definir
  límite estricto de tamaño, plazo, redirecciones y tratamiento de proxy antes
  de implementar; fallos se muestran sin fallback remoto ni instalación.
- La respuesta es **datos no confiables**: esquema cerrado y acotado, versión y
  plataforma validadas, enlaces restringidos a releases oficiales. No ejecutar
  comandos, abrir enlaces automáticamente ni confiar en texto remoto como HTML.
- Comparar versiones con semántica definida de prerelease/canal; una versión
  anterior, igual, malformada o de canal no admitido nunca se anuncia como
  actualización. Versión de producto y SHA de build deben ser distinguibles.
- Un hash junto al paquete detecta corrupción, no demuestra autenticidad frente
  a una fuente comprometida. No prometer actualización segura hasta contar con
  firma verificable y política de claves; nunca incrustar credenciales.

Primer incremento offline: aceptar solo versiones estables numéricas estrictas
`MAJOR.MINOR.PATCH`, sin prefijo, ceros iniciales, prerelease o metadatos.
La comparación es numérica por componente, nunca alfabética. Esta restricción
no define todavía la política de releases experimentales; para estas no se
debe fabricar una actualización estable. Sin dependencia ni I/O.

## Release y migración

Proponer releases experimentales con tag y versión explícitos; no usar el nombre
fijo `0.1.0` para fingir que builds distintos son comparables. Una release debe
proceder de un mismo commit y run exitoso, con payload y SHA256 verificados,
arquitectura y glibc declaradas, notas y enlaces comprobados. Crear borrador
primero; publicación requiere decisión explícita. Paquetes sin firma ni prueba
real de instalación se identifican como tales.

El manifiesto se publicaría solo después de verificar la release y sus activos.
No modificar `main` ni el manifiesto antes de que los activos existan. La
versión previa y datos se conservan: rollback es volver al instalador anterior
sin borrar modelos, historial o configuración; respetar cuarentenas y cierre
confirmado antes de cambiar coordinadores.

Primer contrato offline `stable/v1`: JSON limitado a 4096 bytes, exactamente
`schema`, `version`, `revision` y `release_url`; versión estable estricta,
SHA Git hexadecimal minúsculo de 40 caracteres y URL exacta de la release
GitHub de Seiya correspondiente. No valida existencia, firma ni procedencia.
Ver `contracts/updates/v1/README.md` y fixture no operativa.

## Pendiente de diseño detallado

Ubicación exacta del manifiesto, canal experimental y almacenamiento
persistente del reloj son decisiones pendientes. El workflow de release todavía
no está implementado; los incrementos siguientes se describen por separado.

UP8 propone, además del manifiesto offline independiente, una consulta
explícita a la API fija `/repos/spirock/seiya/releases/latest`: HTTPS, sin
proxy ni redirects, plazo 10 s y cuerpo máximo 64 KiB. Solo acepta respuesta
con `draft=false`, `prerelease=false`, tag `vMAJOR.MINOR.PATCH` y URL exacta
al tag oficial. No interpreta `target_commitish` como SHA, ni demuestra firma
o autenticidad de activos. Fallo de red no debe afectar el uso offline. El
adaptador no se invoca automáticamente ni habilita ninguna UI o endpoint.

UP9 añade un limitador por instancia en memoria, deshabilitado por defecto.
Tras habilitación explícita se reserva antes de iniciar la consulta y no se
reintenta por error hasta transcurridas 24 horas de reloj monotónico. El estado
no sobrevive al reinicio y no limita otras instancias: no equivale aún a una
política persistente global ni implementa la interfaz de consentimiento.

UP10 añade una comprobación manual desde Configuración de modelos: muestra el
aviso de IP antes de solicitarla, requiere pulsación explícita y usa un POST
autenticado de mismo origen. No consulta al abrir, no descarga ni instala.
No se guarda una preferencia ni se ofrece comprobación diaria automática; el
límite de 24 horas solo dura mientras viva esa instancia. La vista previa
sin modelos rechaza la consulta. Validación inicial con fixtures sintéticas;
falta prueba real de navegador/red y publicación.
