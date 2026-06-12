# Sample configuration

Create `config.toml` under `ASTRAL_HOME` (`~/.astral-code` by default). A
minimal provider-neutral setup looks like:

```toml
model = "deepseek-v4-pro"
model_provider = "deepseek"

[model_providers.deepseek]
name = "deepseek"
base_url = "https://api.deepseek.com/v1"
env_key = "ASTRAL_API_KEY"
wire_api = "chat_completions"
```
