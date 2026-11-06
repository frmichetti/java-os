# Spec Delta

## Purpose

Permite ao kernel carregar uma biblioteca compartilhada ELF64 (`ET_DYN`) a partir do sistema de arquivos para dentro de um processo já em execução, resolvendo e aplicando suas relocações e expondo seus símbolos para serem chamados por esse processo — a base de `dlopen`/`dlsym` POSIX.

## ADDED Requirements

### Requirement: Carregamento de Biblioteca Compartilhada a partir de um Caminho no Sistema de Arquivos
O kernel SHALL ser capaz de carregar, dentro do processo chamador, uma biblioteca compartilhada ELF64 (`ET_DYN`) identificada por um caminho absoluto no sistema de arquivos, devolvendo um identificador opaco (`handle`) que referencia essa biblioteca carregada nesse processo.

#### Scenario: Caminho válido para uma biblioteca compartilhada ELF64 válida
- **WHEN** um processo em execução pede o carregamento de um caminho absoluto existente, contendo uma biblioteca compartilhada ELF64 válida
- **THEN** a biblioteca SHALL ser carregada nesse processo e um `handle` SHALL ser devolvido, referenciando-a para chamadas futuras

#### Scenario: Caminho não corresponde a nenhum arquivo existente
- **WHEN** um processo pede o carregamento de um caminho que não corresponde a nenhum arquivo existente no sistema de arquivos
- **THEN** o carregamento SHALL falhar com um erro indicando arquivo não encontrado, sem carregar biblioteca alguma

#### Scenario: Conteúdo não é uma biblioteca compartilhada ELF64 válida
- **WHEN** o conteúdo lido não é um ELF64 válido, ou é um ELF64 válido mas não do tipo `ET_DYN` (biblioteca compartilhada), ou de arquitetura diferente de x86-64
- **THEN** o carregamento SHALL falhar com um erro indicando formato ou tipo não suportado, sem mapear memória nenhuma

### Requirement: Carregamento de Dependências Declaradas antes da Biblioteca Dependente
Antes de aplicar as relocações de uma biblioteca, o kernel SHALL carregar, recursivamente, cada biblioteca da qual ela declara depender (`DT_NEEDED`), de forma que os símbolos de uma dependência já estejam disponíveis para a resolução das relocações da biblioteca que depende dela.

#### Scenario: Biblioteca com uma dependência declarada
- **WHEN** uma biblioteca carregada declara depender de outra biblioteca, ainda não carregada no processo
- **THEN** a dependência SHALL ser carregada, com suas próprias relocações aplicadas e seus símbolos disponíveis, antes de qualquer relocação da biblioteca dependente ser aplicada

#### Scenario: Dependência declarada não é encontrada
- **WHEN** uma biblioteca declara depender de outra que não existe no sistema de arquivos no caminho esperado
- **THEN** o carregamento SHALL falhar com um erro indicando dependência não encontrada, sem deixar a biblioteca dependente parcialmente carregada

### Requirement: Resolução e Aplicação de Relocações ao Carregar
Ao carregar uma biblioteca, o kernel SHALL resolver e aplicar, de forma imediata (sem postergar para o primeiro uso), cada relocação declarada por ela, escrevendo o valor resolvido no endereço de memória que a relocação indica — cobrindo ao menos relocações relativas à base de carregamento e relocações que referenciam um símbolo por nome.

#### Scenario: Relocação relativa à base de carregamento
- **WHEN** uma biblioteca declara uma relocação cujo valor é relativo ao endereço de base em que ela foi carregada
- **THEN** o kernel SHALL escrever, no endereço indicado pela relocação, a soma da base de carregamento com o deslocamento declarado

#### Scenario: Relocação que referencia um símbolo definido em outra biblioteca já carregada
- **WHEN** uma biblioteca declara uma relocação que referencia, por nome, um símbolo definido por outra biblioteca já carregada no mesmo processo
- **THEN** o kernel SHALL escrever, no endereço indicado pela relocação, o endereço resolvido desse símbolo

#### Scenario: Relocação referencia um símbolo que não existe em nenhuma biblioteca carregada
- **WHEN** uma relocação referencia, por nome, um símbolo que não é encontrado em nenhuma biblioteca já carregada no processo (incluindo as dependências recém-carregadas)
- **THEN** o carregamento SHALL falhar com um erro indicando símbolo não resolvido, sem deixar a biblioteca parcialmente carregada e utilizável

### Requirement: Resolução de Símbolo por Nome a partir de um Handle de Biblioteca
O kernel SHALL ser capaz de resolver o endereço de um símbolo exportado por uma biblioteca já carregada, dados o `handle` dessa biblioteca e o nome do símbolo.

#### Scenario: Símbolo existente na biblioteca referenciada pelo handle
- **WHEN** um processo pede a resolução de um nome de símbolo que a biblioteca referenciada pelo `handle` exporta
- **THEN** o endereço em que esse símbolo foi carregado SHALL ser devolvido

#### Scenario: Símbolo não existe na biblioteca referenciada
- **WHEN** um processo pede a resolução de um nome de símbolo que a biblioteca referenciada pelo `handle` não exporta
- **THEN** a resolução SHALL falhar com um erro indicando símbolo não encontrado

#### Scenario: Handle não corresponde a nenhuma biblioteca carregada nesse processo
- **WHEN** um processo pede a resolução de um símbolo usando um `handle` que não corresponde a nenhuma biblioteca carregada por ele
- **THEN** a resolução SHALL falhar com um erro indicando `handle` inválido

### Requirement: Isolamento de Bibliotecas Carregadas entre Processos Diferentes
Bibliotecas carregadas por um processo, e os `handles` que as referenciam, SHALL ser específicos desse processo: não SHALL ser visíveis nem resolvíveis a partir de outro processo, mesmo que ambos tenham carregado o mesmo caminho de arquivo.

#### Scenario: Dois processos carregam a mesma biblioteca independentemente
- **WHEN** dois processos diferentes carregam, cada um, uma biblioteca a partir do mesmo caminho no sistema de arquivos
- **THEN** cada processo SHALL receber seu próprio `handle`, independente do outro, e um `handle` obtido em um processo SHALL ser inválido quando usado a partir do outro

### Requirement: Falha de Carregamento ou Resolução Não Compromete a Estabilidade do Kernel
Qualquer falha durante o carregamento de uma biblioteca ou a resolução de um símbolo (arquivo ilegível, formato inválido, dependência não encontrada, símbolo não resolvido, falta de memória ou de espaço de endereçamento virtual) SHALL ser reportada como um erro ao chamador, sem travar, reiniciar ou corromper o estado do kernel ou de outros processos já em execução.

#### Scenario: Falta de memória ao mapear os segmentos de uma biblioteca
- **WHEN** não há memória física suficiente para mapear os segmentos de uma biblioteca sendo carregada
- **THEN** o carregamento SHALL falhar com um erro indicando falta de memória, e o kernel SHALL continuar operacional, sem afetar processos já em execução
