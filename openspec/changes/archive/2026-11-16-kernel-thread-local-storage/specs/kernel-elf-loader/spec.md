# Spec Delta

## ADDED Requirements

### Requirement: Reconhecimento do Segmento de Armazenamento Local de Thread (TLS)
Ao interpretar os segmentos de um executável, o kernel SHALL reconhecer um
segmento de TLS (`PT_TLS`), quando presente, extraindo seu tamanho em
memória, seu alinhamento, e sua imagem inicial (os bytes do arquivo
correspondentes a esse segmento), para uso na inicialização do bloco de TLS
de cada thread desse processo.

#### Scenario: Executável com segmento de TLS tem seus dados extraídos
- **WHEN** um executável válido declara um segmento `PT_TLS`
- **THEN** o kernel SHALL disponibilizar o tamanho em memória, o
  alinhamento, e a imagem inicial declarados por esse segmento

#### Scenario: Imagem de TLS menor que o tamanho em memória é complementada com zero
- **WHEN** o segmento `PT_TLS` de um executável declara um tamanho em
  memória maior que o tamanho de sua imagem inicial no arquivo
- **THEN** o kernel SHALL tratar o espaço além da imagem inicial, até o
  tamanho em memória declarado, como implicitamente zerado — mesma
  convenção já usada para a área de BSS de um segmento `PT_LOAD`

#### Scenario: Executável sem segmento de TLS não é afetado
- **WHEN** um executável válido não declara nenhum segmento `PT_TLS`
- **THEN** o carregamento SHALL prosseguir exatamente como antes desta
  capacidade existir, sem nenhuma imagem de TLS disponibilizada
