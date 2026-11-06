# Spec Delta

## Purpose

Permite ao kernel carregar um executável ELF64 estático lido do sistema de arquivos e colocá-lo para rodar em um novo processo de modo usuário, sem exigir que o código do programa já esteja embutido no binário do kernel.

## ADDED Requirements

### Requirement: Carregamento de Executável a partir de um Caminho no Sistema de Arquivos
O kernel SHALL ser capaz de criar um novo processo a partir de um executável ELF64 estático identificado por um caminho absoluto no sistema de arquivos, lendo seu conteúdo antes de interpretá-lo.

#### Scenario: Caminho válido para um executável ELF64 estático
- **WHEN** o kernel recebe o caminho absoluto de um arquivo existente contendo um executável ELF64 estático válido
- **THEN** um novo processo SHALL ser criado a partir do conteúdo lido desse arquivo

#### Scenario: Caminho não corresponde a nenhum arquivo existente
- **WHEN** o kernel recebe um caminho que não corresponde a nenhum arquivo existente no sistema de arquivos
- **THEN** o carregamento SHALL falhar com um erro indicando arquivo não encontrado, sem criar processo algum

### Requirement: Validação do Cabeçalho e da Arquitetura do Executável
Antes de mapear qualquer segmento, o kernel SHALL validar que o conteúdo lido é um executável ELF64 estático (`ET_EXEC`) para a arquitetura x86-64, rejeitando qualquer outro formato ou arquitetura sem mapear memória nenhuma.

#### Scenario: Conteúdo não é um arquivo ELF válido
- **WHEN** os bytes lidos não começam com a assinatura ELF (`0x7F 'E' 'L' 'F'`) esperada
- **THEN** o carregamento SHALL falhar com um erro indicando formato inválido, sem criar processo nem mapear memória

#### Scenario: ELF válido mas de classe, endianness, arquitetura ou tipo não suportados
- **WHEN** o cabeçalho ELF é válido mas declara uma classe diferente de 64 bits, endianness diferente de little-endian, arquitetura diferente de x86-64, ou tipo diferente de executável estático (`ET_EXEC`)
- **THEN** o carregamento SHALL falhar com um erro indicando a característica não suportada, sem criar processo nem mapear memória

### Requirement: Mapeamento dos Segmentos Carregáveis com Permissões Derivadas do Executável
Para um executável válido, o kernel SHALL mapear cada segmento carregável (`PT_LOAD`) descrito pelo executável em páginas de memória do novo processo, com permissões de leitura, escrita e execução derivadas das flags desse segmento no executável, e com a porção além do conteúdo do arquivo (quando o tamanho em memória do segmento exceder seu tamanho no arquivo) zerada.

#### Scenario: Segmento executável e somente leitura
- **WHEN** um segmento carregável do executável está marcado como legível e executável, mas não gravável
- **THEN** as páginas mapeadas para esse segmento SHALL ser legíveis e executáveis, e não SHALL permitir escrita a partir do próprio processo

#### Scenario: Segmento de dados com área de BSS
- **WHEN** um segmento carregável do executável tem um tamanho em memória maior que seu tamanho no arquivo
- **THEN** os bytes do arquivo SHALL ser copiados no início do segmento e todo o espaço restante, até o tamanho em memória declarado, SHALL ser preenchido com zero

### Requirement: Transferência de Controle para o Ponto de Entrada do Executável
Após o mapeamento de todos os segmentos carregáveis, o kernel SHALL criar a thread inicial do novo processo em modo usuário, com uma stack de usuário própria, iniciando sua execução exatamente no ponto de entrada declarado pelo executável.

#### Scenario: Execução começa no ponto de entrada declarado
- **WHEN** um executável válido é carregado com sucesso
- **THEN** a primeira instrução executada pela thread inicial do novo processo SHALL ser a que está no endereço de ponto de entrada declarado no cabeçalho do executável

### Requirement: Falha de Carregamento Não Compromete a Estabilidade do Kernel
Qualquer falha durante o carregamento de um executável (arquivo ilegível, formato inválido, segmento malformado ou sobreposto, ou falta de memória para mapear os segmentos) SHALL ser reportada como um erro ao chamador, sem travar, reiniciar ou corromper o estado do kernel ou de outros processos já em execução.

#### Scenario: Falha de alocação de memória durante o mapeamento de um segmento
- **WHEN** não há memória física suficiente para mapear um dos segmentos carregáveis do executável
- **THEN** o carregamento SHALL falhar com um erro indicando falta de memória, e o kernel SHALL continuar operacional, sem afetar processos já em execução

#### Scenario: Segmentos carregáveis se sobrepõem
- **WHEN** dois segmentos carregáveis do executável, mapeados em seus endereços virtuais declarados, ocupariam a mesma página de memória
- **THEN** o carregamento SHALL falhar com um erro indicando sobreposição, sem deixar o novo processo parcialmente criado em execução
