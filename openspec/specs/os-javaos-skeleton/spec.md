# os-javaos-skeleton Specification

## Purpose

Fornece o esqueleto inicial da camada de abstração de SO que o HotSpot exige de todo porte (`src/hotspot/os/<plataforma>` e `src/hotspot/os_cpu/<plataforma>_<variante>`) para o Java OS — arquivos que seguem a assinatura real da interface do HotSpot e compilam contra o código-fonte vendorizado do OpenJDK usando o toolchain já existente do projeto, sem ainda integrar com o sistema de build do próprio OpenJDK nem executar bytecode.

## Requirements

### Requirement: Arquivos do Port Seguem a Assinatura Real da Interface do HotSpot
Cada arquivo novo em `os/javaos`/`os_cpu/javaos_zero` SHALL declarar e definir as funções exigidas pela interface real do HotSpot para esse arquivo (mesma assinatura — nome, parâmetros, tipo de retorno — da interface correspondente em `share/runtime/os.hpp` ou do arquivo análogo de uma plataforma já suportada), mesmo que o corpo de cada função seja um stub mínimo.

#### Scenario: Função do port corresponde à assinatura exigida pela interface
- **WHEN** a interface do HotSpot declara uma função que uma implementação de porte deve fornecer
- **THEN** o arquivo correspondente em `os/javaos`/`os_cpu/javaos_zero` SHALL definir uma função com exatamente o mesmo nome, parâmetros e tipo de retorno

### Requirement: Arquivos do Port Compilam Contra o Código-Fonte Real do HotSpot
Um arquivo de `os/javaos`/`os_cpu/javaos_zero` que o processo de build desta etapa relate como "compilado com sucesso" SHALL ter sido de fato compilado pelo toolchain do projeto diretamente contra os headers reais do HotSpot vendorizado (não um header reescrito ou simplificado por este projeto), sem erros de compilação.

#### Scenario: Compilação bem-sucedida não omite nenhum header real exigido
- **WHEN** um arquivo de `os/javaos`/`os_cpu/javaos_zero` é compilado como parte da verificação desta change
- **THEN** a compilação SHALL incluir, via caminho de busca de *header*, os arquivos reais e inalterados do HotSpot vendorizado que esse arquivo inclui, em vez de uma versão substituída ou simplificada por este projeto

#### Scenario: Arquivo que não compila é relatado como tal, não omitido
- **WHEN** um arquivo planejado para esta change não consegue compilar contra os headers reais (por exemplo, por depender de outra parte do código compartilhado do HotSpot ainda não coberta)
- **THEN** essa falha SHALL ser registrada explicitamente na documentação desta change, em vez de omitida ou contada como sucesso
