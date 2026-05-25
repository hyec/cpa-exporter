package collector

import "context"

type Collector interface {
	Run(ctx context.Context)
}
