package adapters

import (
	"net/http"

	oresmiddleware "github.com/ORESoftware/ores-middleware/src/golang"
	"github.com/gin-gonic/gin"
	"github.com/gofiber/fiber/v2"
	"github.com/gofiber/fiber/v2/middleware/adaptor"
	"github.com/gorilla/mux"
	"github.com/labstack/echo/v4"
)

func NetHTTP(stack *oresmiddleware.Stack, handler http.Handler) http.Handler {
	return stack.Wrap(handler)
}
func GorillaMux(stack *oresmiddleware.Stack, router *mux.Router) http.Handler {
	return stack.Wrap(router)
}
func Gin(stack *oresmiddleware.Stack, engine *gin.Engine) http.Handler { return stack.Wrap(engine) }
func Echo(stack *oresmiddleware.Stack, engine *echo.Echo) http.Handler { return stack.Wrap(engine) }
func Fiber(stack *oresmiddleware.Stack, app *fiber.App) http.Handler {
	return stack.Wrap(adaptor.FiberApp(app))
}
