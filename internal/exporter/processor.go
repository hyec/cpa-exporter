package exporter

type Processor struct {
	metrics *Metrics
}

func NewProcessor(metrics *Metrics) *Processor {
	return &Processor{metrics: metrics}
}

func (p *Processor) ProcessPayload(payload []byte) bool {
	record, err := DecodeUsageRecord(payload)
	if err != nil {
		p.metrics.DecodeError()
		return false
	}
	p.metrics.Observe(record)
	return true
}

func (p *Processor) Metrics() *Metrics {
	return p.metrics
}
