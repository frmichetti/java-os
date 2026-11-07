# Spec Delta

## ADDED Requirements

### Requirement: Variável de Condição com Espera Limitada por Prazo
O kernel SHALL fornecer uma variante da espera em variável de condição que aceita um
prazo máximo, liberando e reaquirindo o mutex associado com a mesma atomicidade da
espera sem prazo, e retornando quando notificada ou quando o prazo expirar, o que
ocorrer primeiro.

#### Scenario: Espera com prazo notificada antes de expirar reaquire o mutex normalmente
- **WHEN** uma thread chama a espera com prazo, detendo um mutex no momento da
  chamada, e é notificada antes do prazo expirar
- **THEN** a chamada SHALL retornar indicando que foi notificada, com o mesmo mutex
  associado novamente detido pela thread chamadora

#### Scenario: Espera com prazo que expira sem notificação também reaquire o mutex
- **WHEN** uma thread chama a espera com prazo, detendo um mutex no momento da
  chamada, e nenhuma notificação a alcança antes do prazo expirar
- **THEN** a chamada SHALL retornar indicando que o prazo expirou, com o mesmo mutex
  associado novamente detido pela thread chamadora (mesma garantia de reaquisição da
  espera sem prazo)
