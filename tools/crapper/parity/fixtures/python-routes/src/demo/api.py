def build_router(config):
    router = APIRouter()
    if config.debug:
        router.debug = True

    @router.get("/users/{user_id}", response_model=User)
    async def user(user_id: str):
        if not user_id:
            return None
        return user_id

    @router.post("/users", include_in_schema=config.debug or config.docs)
    @requires("admin")
    async def add_user(body):
        return body if body else None

    @router.api_route(path="/sync", methods=["GET", "post"])
    def sync():
        return 1

    @router.get("/users")
    def users_again():
        return 1

    @router.get("/users")
    def users_third():
        return 1

    return router


def decorate(fn):
    @mock.patch("pkg.mod.attr")
    def patched(mocked):
        return 1 if mocked else 0

    return patched


@app.get("/health")
def health():
    return 1
